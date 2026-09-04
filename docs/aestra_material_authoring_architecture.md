# Aestra Material Authoring Architecture

## Status

Proposed design for Aestra's material and shader authoring system, revised after
the completed [current material-system audit](material-system/current-state.md).

The goal is to support professional VFX material creation while making **humans and AI first-class authors**. Aestra should not make a traditional shader node graph the canonical representation. Instead, it should use a **typed semantic expression DAG** as the source of truth, with multiple projections for different users and workflows.

---

# 1. Design Goals

Aestra's material system should:

- support professional real-time VFX materials;
- work naturally with Aestra's existing semantic authoring architecture;
- be easy for AI to inspect and modify safely;
- remain pleasant for VFX artists who expect graphical tools;
- allow simple materials to be authored without exposing shader programming;
- support advanced procedural materials without limiting expert users;
- separate reusable shader logic from per-effect material values;
- allow deterministic compilation and preview;
- expose material parameters to emitters, effect clips, automation, and runtime systems;
- support undo/redo and transactional edits through semantic commands;
- keep editor-only graph layout metadata separate from shader semantics;
- allow future renderer backends without coupling authoring data to one GPU language;
- compile efficiently to WESL/WGSL/wgpu-oriented runtime code;
- support validation, reflection, optimization, diagnostics, and migration.

The core principle is:

> **The node graph is a visualization and editing surface, not the source of truth.**

---

# 2. Why a Traditional Shader Graph Should Not Be Canonical

Traditional material editors usually persist a graph containing:

- node IDs;
- socket IDs;
- graph positions;
- links;
- reroute nodes;
- comments;
- frames;
- UI coordinates;
- editor-specific metadata.

That representation works well for a human visual editor but is poor for AI.

For example, an AI should not need to understand that:

```json
{
  "node_id": 72,
  "position": [423.2, -218.4],
  "output": "A",
  "connected_to": 41
}
```

means:

```text
noise * 0.35
```

Node graphs create unnecessary problems for AI authoring:

- unstable IDs;
- fragile connection editing;
- irrelevant layout state;
- excessive serialization noise;
- difficult semantic diffing;
- poor readability;
- awkward programmatic transformations;
- difficult migrations;
- unnecessary dependence on a specific graph editor.

Aestra should instead expose the **meaning** of the material.

---

# 3. Proposed Architecture

```text
                    HUMAN
                      │
        ┌─────────────┼─────────────┐
        │             │             │
    Inspector      Layers        Graph
        │             │             │
        └─────────────┼─────────────┘
                      │
                      ▼
                Material AST
               SOURCE OF TRUTH
                      ▲
                      │
                     AI
                      │
          Semantic Commands / DSL
                      │
                      ▼
              Material Compiler
                      │
              ┌───────┴────────┐
              ▼                ▼
             WESL         Reflection /
                           Diagnostics
              │
              ▼
             GPU
```

The same material can therefore have several projections:

```text
                 Material AST
               /      |      \
              /       |       \
       Inspector    Graph     Text
           ↑          ↑         ↑
         Human      Human    Human / AI
```

No projection owns the material.

---

# 4. Material Program and Material Instance

Aestra should separate **shader logic** from **effect-specific parameter values**.

The ownership model is:

```text
Project / built-ins
 └─ MaterialProgram (stable MaterialProgramId, schema version)

EffectAsset
 ├─ MaterialInstance (stable MaterialId inside the effect)
 │   ├─ MaterialProgramRef
 │   └─ typed values/resources keyed by MaterialParameterId
 └─ Emitter
     └─ Renderer
         └─ MaterialId
```

`MaterialProgramId`, `MaterialParameterId`, `MaterialExpressionId`, `MaterialId`,
and referenced `AssetId`s are semantic identities. Names and filesystem paths are
only display and location metadata and must never be used as binding keys.

```rust
pub enum MaterialProgramRef {
    BuiltIn(MaterialProgramId),
    Project(MaterialProgramId),
}
```

Project lookup maps a stable program ID to its current asset location. Renaming
or moving a program therefore does not rewrite effects.

## 4.1 Material Program

A `MaterialProgram` defines how pixels and vertices are produced. It owns:

- the material domain;
- typed parameter declarations and defaults;
- the semantic expression graph and outputs;
- the resource interface;
- the render-state policy and defaults;
- its schema version.

Examples:

- UV manipulation;
- texture sampling;
- dissolve;
- distortion;
- Fresnel;
- depth fade;
- emissive calculation;
- particle input usage;
- vertex displacement.

Conceptually:

```text
MaterialProgram "MagicFlame"

Flame Texture
     │
     ├─────────────┐
     ▼             │
  Sample           │
     │             │
     ▼             │
   Color           │
                   │
Noise Texture      │
     │             │
     ▼             │
    Pan ← Time     │
     │             │
     ▼             │
  Sample           │
     │             │
     ▼             │
 Distortion ───────┘

Particle Color
     │
     ▼
 Multiply
     │
     ▼
 Fresnel
     │
     ▼
 Output
```

## 4.2 Material Instance

A `MaterialInstance` references a program and supplies concrete values and
resources. It is reusable within an effect and is the object referenced by an
emitter renderer.

Example:

```ron
MaterialInstance(
    id: MaterialId("0196..."),
    program: Project(MaterialProgramId("0195...")),
    values: {
        MaterialParameterId("0194...01"): Asset(AssetId("0193...11")),
        MaterialParameterId("0194...02"): Asset(AssetId("0193...12")),
        MaterialParameterId("0194...03"): Constant(Float(0.35)),
        MaterialParameterId("0194...04"): Constant(ColorSrgb(0.22, 0.41, 1.0, 1.0)),
        MaterialParameterId("0194...05"): EffectParameter(ParameterId("0192...07")),
    },
    render_state: (blend: Additive, ..),
)
```

Many emitters can reuse the same program with different instances.

This is important for:

- runtime batching;
- asset reuse;
- effect libraries;
- variant creation;
- AI editing;
- avoiding unnecessary shader recompilation.

For example, the request:

> Make the flame more turbulent and slightly bluer.

should usually change only a `MaterialInstance`:

```text
distortion = 0.55
tint = #80aaff
```

The request:

> Add glowing Fresnel edges.

changes the `MaterialProgram`.

Ordinary instance values and texture assignments never trigger shader
compilation. A render-state edit may select or create a pipeline variant, but
does not change shader logic. Static shader specialization must be declared
explicitly by the program; it must not emerge accidentally from a normal
instance edit.

Duplicating, exploding, or migrating effects must preserve stable program and
asset references and remap only effect-local `MaterialId`s when required.

## 4.3 Specialization Contract

Program declarations classify each configurable item so edits have predictable
cost and ownership:

| Class | Owned by | Example | Runtime consequence |
|---|---|---|---|
| Shader-static | Program declaration + explicit instance specialization | optional depth-fade code path | program fingerprint/shader variant |
| Pipeline-static | Instance render state within program policy | blend or depth mode | pipeline cache lookup/build |
| Instance dynamic | Material instance | tint, intensity, texture asset | uniform/resource update |
| Effect/emitter dynamic | Effect system binding | automated distortion | value update at declared rate |
| Particle/vertex/fragment | Program expression input | particle age, UV, scene depth | shader evaluation |

`MaterialRenderStatePolicy` supplies a default and the set of permitted concrete
states. An instance cannot request an undeclared state. Shader-static parameters
are opt-in and separately identified; a compiler must never specialize an
ordinary dynamic value merely because it happens to be constant today.

---

# 5. Canonical Representation: Typed Semantic Expression DAG

The canonical representation is a typed semantic expression DAG, not source
text and not an editor graph. "AST" remains acceptable shorthand, but the model
deliberately permits shared subexpressions.

Possible structure:

```rust
pub struct MaterialProgram {
    pub id: MaterialProgramId,
    pub schema_version: MaterialSchemaVersion,
    pub name: String,
    pub domain: MaterialDomain,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub parameters: Vec<MaterialParameter>,
    pub expressions: Vec<MaterialExpression>,
    pub outputs: MaterialOutputs,
}

pub struct MaterialExpression {
    pub id: MaterialExpressionId,
    pub kind: MaterialExpressionKind,
}

pub struct MaterialOutputs {
    pub color: MaterialExpressionId,
    pub alpha: MaterialExpressionId,
}
```

Example domains:

```rust
pub enum MaterialDomain {
    Sprite,
    Mesh,
    Ribbon,
    Decal,
    Screen,
}
```

Example render state:

```rust
pub struct MaterialRenderState {
    pub blend: BlendMode,
    pub depth_test: DepthTest,
    pub depth_write: bool,
    pub cull_mode: CullMode,
}
```

Example parameter types:

```rust
pub enum MaterialValueType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Texture2D,
    Bool,
}
```

Expressions should use stable semantic IDs internally, but those IDs must not carry editor-layout meaning.

Expression operands reference `MaterialExpressionId`s. The expression vector is
serialized in a deterministic normalized order. Validation rejects missing
references, duplicate IDs, cycles, and invalid output roots. Shared
subexpressions are valid. Replacing an expression changes every consumer;
rewiring a `MaterialConnection` changes only the specified consumer input.
Unreachable expressions are a diagnostic and may be removed through a semantic
command.

## 5.1 Color and Alpha Convention

Aestra uses these conventions end to end:

- color literals are displayed and serialized as sRGB values, then converted to
  linear RGBA for compilation and evaluation;
- texture parameters declare `SrgbColor` or `LinearData`; this is never guessed
  by the backend;
- the `Color` output is finite linear RGB and may be HDR;
- the `Alpha` output is a `Float`, saturated to `[0, 1]` at the final material
  boundary;
- material output is straight alpha; no expression is implicitly premultiplied;
- the selected blend mode owns the final blend equation and preserves the
  current renderer's compatibility behavior;
- sampler filtering, addressing, and mip policy are explicit resource metadata.

These conventions prevent the authoring model, compiler, and renderer from
silently applying different color transforms.

---

# 6. Textual Material Representation

A textual representation should exist because it is:

- readable;
- diffable;
- version-control friendly;
- AI friendly;
- useful for debugging;
- useful for expert users.

The typed model is canonical in memory. Canonical normalized RON is the initial
persistent representation. A future DSL is an import/export and editing
projection that must parse into the typed model, validate, and serialize back
without creating hidden semantic state.

A future Aestra material DSL could look like:

```text
material MagicFlame {
    domain: Sprite
    blend: Additive

    parameter tint: Color = #ff7020
    parameter distortion: Float = 0.35
    parameter intensity: Float = 4.0

    texture flame
    texture noise

    uv animated_uv =
        pan(uv0, vec2(0.0, -0.4) * effect_time)

    float noise_value =
        sample(noise, animated_uv * 2.0).r

    color flame_color =
        sample(flame, uv0)

    output.color =
        flame_color.rgb
        * particle.color.rgb
        * tint
        * intensity
        * (1.0 + noise_value)

    output.alpha =
        flame_color.a
        * particle.opacity
}
```

Initially, Aestra does not necessarily need a custom parser. RON can be used while the semantic model stabilizes.

The custom DSL should be introduced only if it clearly improves authoring.

---

# 7. Semantic Authoring Commands

AI should preferably modify materials through **semantic commands**, not string patches.

Example commands:

```rust
SetMaterialInstanceParameter {
    material,
    parameter: MaterialParameterId("0194...03"),
    value: MaterialParameterValue::Constant(Float(0.55)),
}
```

```rust
SetMaterialInstanceParameter {
    material,
    parameter: MaterialParameterId("0194...05"),
    value: MaterialParameterValue::EffectParameter(ParameterId("0192...07")),
}
```

```rust
InsertMaterialOperation {
    program,
    target: MaterialOutput::Color,
    operation: MaterialOperation::Fresnel {
        power: 3.0,
    },
}
```

```rust
ReplaceMaterialExpression {
    program,
    expression: MaterialExpressionId("0194...20"),
    replacement,
}
```

```rust
WrapMaterialExpression {
    program,
    expression: MaterialExpressionId("0194...20"),
    wrapper: MaterialOperation::Pow(2.0),
}
```

These operations should integrate with Aestra's existing authoring philosophy:

```text
Editor / AI
     │
     ▼
Semantic command
     │
     ▼
Validation
     │
     ▼
Transaction
     │
     ├─ undo
     ├─ redo
     ├─ diff
     └─ diagnostics
     │
     ▼
Material AST
```

The baseline command set must exist before migration or editor work:

```text
Add / Remove / Replace MaterialProgram
Add / Remove / Replace MaterialInstance
SetMaterialInstanceParameter
SetMaterialInstanceRenderState
AssignRendererMaterial
SetMaterialOutput
Add / Remove / Replace MaterialExpression
RewireMaterialExpressionInput
```

Every mutation, including migrations, must go through this transactional API.
Higher-level commands such as `WrapMaterialExpression` and presets can be added
later as compositions of the baseline commands.

---

# 8. Material Inputs

The material language should expose semantic inputs rather than requiring low-level buffer knowledge.

## Geometry

```text
UV0
UV1
LocalPosition
WorldPosition
Normal
Tangent
ViewDirection
ScreenUV
```

## Particle

```text
ParticleColor
ParticleOpacity
ParticleAge
ParticleNormalizedAge
ParticleLifetime
ParticleVelocity
ParticleSpeed
ParticleRandom
ParticleId
ParticleSize
ParticleRotation
```

## Effect / Emitter

```text
EffectTime
EmitterTime
EffectNormalizedTime
EmitterNormalizedTime
EffectParameter(ParameterId)
EmitterParameter(ParameterId)
```

## Camera / Scene

```text
SceneDepth
CameraPosition
CameraDirection
PixelDepth
```

Support can expand gradually.

## 8.1 Evaluation Domains

Every value has an explicit evaluation domain. The compiler may promote a value
to a later domain, but may not implicitly read a later-domain value earlier.

```text
ShaderStatic  compile-time specialization declared by the program
Instance      material-instance value or resource binding
Effect        value sampled once per effect update
Emitter       value sampled once per emitter update
Particle      value stored or computed per particle
Vertex        value evaluated by the vertex stage
Fragment      value evaluated by the fragment stage
```

`MaterialParameter` represents instance data, an effect/emitter parameter
binding, or an explicitly declared shader-static specialization. Particle
attributes are semantic `MaterialInput`s in the expression DAG, not parameter
sources. Automation is resolved by Aestra's effect system at its declared
effect or emitter rate before the value reaches material binding.

`RandomRange` must declare a domain. Instance/effect/emitter random values are
resolved by the runtime with deterministic seeds. Per-particle randomness uses
`ParticleRandom`; it is not represented as an instance parameter value.
`Expression` is never a parameter source because expressions already belong to
the program DAG.

The first vertical slice supports `Constant` and `EffectParameter` instance
values plus semantic particle and effect-time inputs. Reflection must report
each parameter's type and evaluation domain along with required attributes,
scene inputs, and resources.

---

# 9. Material Operations

Aestra should expose both generic mathematical operations and high-level VFX primitives.

## 9.1 Math

```text
Add
Subtract
Multiply
Divide
Power

Min
Max
Clamp
Saturate
Abs

Floor
Ceil
Round
Fract

Sin
Cos

Step
Smoothstep
Remap
Lerp
```

## 9.2 Vector

```text
Dot
Cross
Normalize
Length
Distance
Append
Split
```

## 9.3 Texture / UV

```text
SampleTexture
SampleGradient

PanUV
RotateUV
ScaleUV
PolarCoordinates
Twirl
FlowMap
DistortUV
Flipbook
Triplanar
```

## 9.4 Noise

```text
ValueNoise
SimplexNoise
PerlinNoise
Voronoi
FBM
```

These may initially lower to library functions instead of being implemented as primitive compiler instructions.

## 9.5 VFX-Specific Primitives

This is where Aestra can differentiate itself.

Examples:

```text
SoftParticle
DepthFade
Fresnel
RadialMask
LinearMask
SphereMask
Dissolve
DissolveEdge
FlowMap
Twirl
PolarWarp
DistanceField
CameraFade
RimLight
VertexOffset
HeatDistortion
```

An AI should be able to express:

```text
Dissolve(
    source: noise,
    threshold: particle.normalized_age,
    edge_width: 0.08
)
```

instead of manually creating a chain of primitive graph nodes.

High-level semantic operations improve:

- AI reliability;
- artist ergonomics;
- readability;
- optimization;
- migrations;
- portability.

---

# 10. Material Inspector

The default artist workflow should not require a node graph.

A typical material could appear as:

```text
MAGIC FLAME
────────────────────────────

Surface
  Domain       Sprite
  Blend        Additive
  Depth        Read only
  Soft Particle ✓

Base
  Texture      flame_02
  Color        Particle Color × [orange]
  Intensity    4.2

UV
  Tiling       1.0  2.0

  Pan
    X           0
    Y          -0.40

  Distortion
    Texture     noise_04
    Amount      0.35
    Speed       0.8

Mask
  Flame Alpha
    × Noise
    × Particle Opacity

Edge
  Fresnel
    Power       3.0
    Color       yellow

────────────────────────────
Graph | Code | Advanced
```

This should cover a large percentage of everyday VFX authoring.

---

# 11. Material Stack / Layer View

Aestra should strongly consider a Photoshop/Substance-style modifier stack.

Example:

```text
Material: Arcane Flame

Surface
Base Texture
UV Pan
Noise Distortion
Dissolve
Fresnel
Color Grade
Soft Particle
```

Each operation is reorderable when semantically valid.

Example:

```text
≡ Base Texture
≡ UV Pan
≡ Noise Distortion
≡ Dissolve
≡ Fresnel
≡ Soft Particle

+ Add Modifier
```

The stack is easier than a graph for:

- common materials;
- quick iteration;
- AI-generated modifications;
- templates/presets;
- learning;
- effect library reuse.

Complex AST structures that cannot be represented cleanly as a stack can automatically switch to an advanced representation.

Implementation status (2026-09-02): the compiler now exposes a deterministic, engine-neutral
read-only stack projection. Reachable semantic operations keep their stable expression IDs and are
ordered from source to output. Properties displays that order for linear programs. Programs with
multiple semantic roots, fan-in, or fan-out report an explicit Advanced fallback; the editor never
pretends that a branched graph is safely reorderable. Modifier editing remains planned.

---

# 12. Node Graph as an Advanced Projection

Aestra should still provide a node graph because graphs are excellent for:

- understanding complex data flow;
- branching masks;
- procedural materials;
- debugging;
- material functions;
- expert editing.

The graph is generated from the AST.

Example:

```text
Noise
  │
 Pan ← Time
  │
Sample
  │
Remap
  │
  × ← Flame Alpha
  │
Alpha Output
```

Graph positions are editor-only metadata:

```rust
pub struct MaterialGraphMetadata {
    pub node_positions: HashMap<MaterialExpressionId, Vec2>,
    pub comments: Vec<GraphComment>,
    pub collapsed_nodes: HashSet<MaterialExpressionId>,
}
```

This metadata must never affect shader semantics.

Implementation status (2026-09-04): a versioned project-local editor-layout sidecar persists
viewport pan/zoom, stable expression and output-node positions, collapsed state, and node-preview
visibility. It is keyed by semantic material IDs, excluded from asset discovery, prunes stale
expression entries, and never participates in material validation, lowering, fingerprints, or
runtime artifacts.

If AI transforms:

```text
noise * 0.3
```

into:

```text
pow(noise, 2.0) * 0.6
```

the graph can regenerate:

```text
Noise
  │
Pow 2
  │
× 0.6
```

without the AI manipulating graph coordinates.

---

# 13. Graph Layout

Graph layout should support two modes.

## Automatic

Default for generated or AI-modified graphs.

Aestra computes a readable layout based on data flow.

## Manual

Artists can move nodes freely.

Manual node positions are stored as editor metadata.

When the AST changes, Aestra should preserve positions for surviving expressions and automatically place only new expressions.

---

# 14. Compiler Pipeline

Aestra should create a dedicated material compiler.

```text
Material AST
     │
     ▼
Validation
     │
     ▼
Typed Material IR
     │
     ▼
Optimization
     │
     ├─ constant folding
     ├─ dead expression elimination
     ├─ common subexpression elimination
     ├─ static branch removal
     ├─ feature detection
     └─ parameter specialization
     │
     ▼
Backend Lowering
     │
     ▼
WESL
     │
     ▼
wgpu shader pipeline
```

The material compiler should produce:

```rust
CompiledMaterialProgram {
    shader,
    resource_layout,
    uniform_layout,
    parameters,
    required_particle_attributes,
    required_scene_inputs,
    render_state_policy,
    program_fingerprint,
    diagnostics,
}
```

## 14.1 Resource ABI

The current renderer's fixed single-texture/single-sampler bind group cannot
support the first vertical slice. The material compiler therefore emits a
deterministic `MaterialResourceLayout`:

```rust
pub struct MaterialResourceLayout {
    pub textures: Vec<MaterialTextureSlot>,
    pub samplers: Vec<MaterialSamplerSlot>,
    pub uniform_layout: MaterialUniformLayout,
}

pub struct MaterialTextureSlot {
    pub parameter: MaterialParameterId,
    pub value_type: MaterialTextureType,
    pub color_space: MaterialTextureColorSpace,
    pub binding: u32,
}
```

Slots are normalized by stable parameter identity, and generated WESL uses the
emitted binding numbers. `aestra-bevy-render` creates the matching Bevy/wgpu
layout and resolves `AssetId`s to runtime handles; portable artifacts never
contain Bevy handles. Sampler descriptors are explicit and may be shared only
when their descriptors are equal.

Validation checks the emitted layout against `BackendCapabilities`, including
texture, sampler, uniform, and storage limits. A missing or incompatible asset
produces a visible fallback and a semantic diagnostic rather than silently
changing the layout.

## 14.2 Specialization and Pipeline Cache

Cache identity is split deliberately:

```text
MaterialProgramFingerprint =
    normalized typed DAG
  + material domain
  + declared shader-static specialization values
  + MaterialResourceLayout

MaterialPipelineKey =
    MaterialProgramFingerprint
  + concrete MaterialRenderState
  + render-target format
  + sample count
  + view/backend feature variant
```

Instance uniform values, texture asset IDs, effect/emitter bindings, and
automation values are excluded from both keys. A program edit rebuilds shader
code; a render-state or target edit may select/build a pipeline; an ordinary
instance edit only updates data or resource bindings.

---

# 15. Material IR

Do not lower directly from the authoring AST to WESL.

Introduce a small typed IR.

Example:

```rust
enum MaterialIrInstruction {
    Constant(Value),
    Input(MaterialInput),
    Parameter(MaterialParameterId),

    SampleTexture {
        texture: ValueId,
        uv: ValueId,
    },

    Add(ValueId, ValueId),
    Multiply(ValueId, ValueId),
    Lerp(ValueId, ValueId, ValueId),

    Fresnel {
        normal: ValueId,
        view: ValueId,
        power: ValueId,
    },
}
```

Benefits:

- simpler validation;
- backend independence;
- easier optimizations;
- easier testing;
- easier diagnostics;
- easier future CPU preview/reference evaluator;
- easier support for multiple rendering backends.

---

# 16. Reflection and Required Inputs

The compiler should automatically determine what a material needs.

Example:

```text
MagicFlame requires:

Particle:
  Color
  Opacity
  NormalizedAge

Textures:
  main_texture
  noise_texture

Scene:
  Depth

Parameters:
  MaterialParameterId(...): tint: Color @ Instance
  MaterialParameterId(...): distortion: Float @ Instance
  MaterialParameterId(...): intensity: Float @ Effect

Resource layout:
  MaterialParameterId(...): main_texture: Texture2D<SrgbColor> @ binding 3
  MaterialParameterId(...): noise_texture: Texture2D<LinearData> @ binding 5
```

This information is useful for:

- runtime storage planning;
- inspector generation;
- validation;
- emitter compatibility warnings;
- batching;
- AI;
- documentation;
- auto-completion.

---

# 17. Material Parameters and Aestra Automation

Material parameters should be bindable to Aestra's existing effect systems.

A dynamic instance parameter initially supports:

```text
Constant
EffectParameter
EmitterParameter
RandomRange(domain: Instance | Effect | Emitter)
```

Example:

```text
distortion:
    EffectParameter(ParameterId("0192...07"))
```

The referenced effect parameter may itself be automated on the timeline. The
material system consumes its typed current value; it does not duplicate the
automation curve inside `MaterialInstance`.

Per-particle behavior is expressed in the program:

```text
edge_intensity:
    particle.normalized_age
```

This keeps evaluation frequency explicit and makes the material system
integrate with the timeline without creating a second automation model.

---

# 18. Material Functions

Reusable material functions should be supported.

Examples:

```text
functions/
  dissolve_edge
  magical_noise
  flame_distortion
  hologram_scanline
  shield_interference
```

A function should have typed inputs and outputs:

```text
function MagicalNoise(
    uv: Vec2,
    time: Float,
    scale: Float
) -> Float
```

Functions can be:

- built into Aestra;
- project-local;
- library assets;
- marketplace assets later;
- authored by users;
- created or modified by AI.

This is more reusable than copy/pasting graph fragments.

---

# 19. Custom WESL Escape Hatch

A semantic material language will never cover every use case.

Aestra should therefore support custom shader functions.

Example:

```wesl
fn arcane_interference(
    uv: vec2<f32>,
    time: f32
) -> f32 {
    // custom implementation
}
```

The AST can expose it as:

```text
ArcaneInterference(
    uv: uv0,
    time: effect_time
)
```

Custom WESL should be an advanced escape hatch, not the normal authoring format.

It should have explicit metadata:

```rust
CustomMaterialFunction {
    name,
    inputs,
    output,
    source,
}
```

This allows validation and reflection around the custom implementation.

---

# 20. AI Material Authoring

The AI workflow should operate primarily on the semantic representation.

Example request:

> Make this magical shield look more unstable. Add flowing cracks moving upward, with brighter edges near the end of particle life.

Possible AI operations:

```text
1. Inspect material program
2. Find current mask path
3. Add Voronoi crack source
4. Add upward UV pan
5. Multiply crack strength by ParticleNormalizedAge
6. Add emissive edge around crack mask
7. Compile
8. Render preview
9. Inspect contact sheet
10. Refine parameters
```

The AI should prefer semantic commands such as:

```text
Add modifier "Voronoi"
Bind "time" to EffectTime
Bind "edge intensity" to ParticleNormalizedAge
Set "flow direction" to [0, 1]
```

instead of editing shader strings directly.

---

# 21. AI Visual Feedback Loop

Aestra's deterministic preview capabilities should become part of the AI workflow.

```text
AI
 │
 ├─ semantic material patch
 │
 ▼
Material Compiler
 │
 ▼
Preview Renderer
 │
 ▼
Deterministic Frames / Contact Sheet
 │
 ▼
Vision Analysis
 │
 └──────────────→ next semantic patch
```

This enables requests such as:

> Make the burst brighter in the first 100 ms but reduce the visual noise near the end.

The AI can modify, render, inspect, and refine.

This can become one of Aestra's strongest differentiators compared with conventional VFX editors.

---

# 22. Validation and Diagnostics

Material validation should occur before GPU compilation where possible.

Examples:

```text
error:
  Material output Alpha expects Float but received Vec3.

warning:
  SceneDepth is unavailable for this renderer.

warning:
  Texture parameter "noise" is unbound.

warning:
  Material uses ParticleVelocity but emitter does not generate velocity.

warning:
  Expression is unreachable and will be removed.
```

Diagnostics should identify semantic concepts rather than generated WESL line numbers whenever possible.

WESL compiler errors should be mapped back to AST expressions.

---

# 23. Material Diffing

Semantic representation allows useful diffs.

Instead of:

```text
node 34 deleted
node 62 added
edge 12 changed
```

Aestra can show:

```text
MagicFlame

Changed:
  Distortion
    0.35 → 0.55

Added:
  Fresnel
    power: 3.0
    intensity: 2.5

Binding changed:
  edge_intensity
    Constant(1.0)
    → ParticleNormalizedAge
```

This is ideal for:

- undo history;
- AI review;
- source control;
- collaboration;
- asset updates.

---

# 24. Material Presets

Aestra should eventually include semantic presets.

Examples:

```text
Additive Flame
Soft Smoke
Energy Beam
Magic Shield
Dissolve
Hologram
Heat Distortion
Lightning
Ghost
Portal
Trail
Impact Flash
```

Applying a preset inserts semantic material operations rather than copying opaque shader code.

AI can use the same preset system.

Example:

> Give this sprite a hologram look.

could map to:

```text
ApplyPreset("Hologram")
```

followed by parameter tuning.

---

# 25. Repository-Aligned Crate Architecture

The first implementation extends the existing dependency graph rather than
introducing three speculative crates. `EffectAsset`, `MaterialDefinition`, and
renderer material references are already owned by `aestra-core`; moving only
part of that model to an `aestra-material` crate would create a dependency cycle.

```text
aestra-core
  └─ material semantic types, stable IDs, expression DAG, values,
     resource schema, pure semantic validation
          │
          ├──────────────► aestra-authoring
          │                 └─ transactional material commands
          │
          ├──────────────► aestra-compiler
          │                 └─ typed IR, reflection, optimization,
          │                    backend-capability validation
          │                         │
          │                         ▼
          └──────────────► aestra-gpu
                            └─ WESL lowering, resource layout, portable cache keys
                                    │
                                    ▼
                            aestra-bevy-render
                              └─ GPU upload, asset resolution, bind groups, pipelines

aestra-project
  └─ indexes external MaterialProgram and texture dependencies

aestra-artifact
  └─ serializes compiled program/instance DTOs and resource layouts

aestra-editor
  └─ inspector, stack, graph, code projection, preview
```

The semantic model begins in a focused `aestra_core::material` module. Compiler
logic belongs in `aestra_compiler::material`; portable shader lowering belongs
in `aestra_gpu::material`; UI belongs in the actual `aestra-editor` application.
`aestra-bevy-render` is an adapter only and must not own authoring semantics.
Pure type, graph, and domain validation lives beside the model so
`aestra-authoring` commands can validate candidate transactions without a
dependency on the compiler. The compiler adds backend-capability diagnostics.

An `aestra-material` crate may be extracted later only if a lower shared
identity/primitives layer makes the dependency direction acyclic and the split
has a demonstrated compile-time or ownership benefit. Neither `aestra-core`,
`aestra-authoring`, nor `aestra-compiler` may depend on Bevy, wgpu, or editor UI.

---

# 26. Runtime Asset Model

Suggested runtime relationship:

```text
Effect
 └─ Emitter
     └─ Renderer
         └─ MaterialInstance
              └─ MaterialProgram
```

Multiple instances can reference one program:

```text
MagicFlameProgram
   ↑       ↑       ↑
 red      blue    poison
 flame    flame    flame
```

The renderer should hold a material instance/reference rather than embed shader logic directly.

Project-level `MaterialProgram`s are dependencies of effects. Effect-local
`MaterialInstance`s hold typed overrides and resource references. Loading an
effect resolves its program dependencies before compilation; loading an
artifact uses the compiled program reference and resource layout without
reconstructing editor state.

---

# 27. Serialization

The first implementation should favor stability over syntax beauty.

Recommended evolution:

```text
Phase 1
Typed Rust AST + RON serialization

Phase 2
Canonical normalized RON

Phase 3
Optional dedicated Aestra Material DSL

Phase 4
Round-trip text editor
```

Do not block the architecture on designing the perfect DSL.

The distinction is precise: the typed model is canonical in memory, normalized
RON is canonical persistence for the first implementation, and a future DSL is
a projection/import format. Round-tripping any projection must preserve stable
semantic IDs and produce the same normalized model.

---

# 28. Compatibility and Versioning

Material assets should carry schema versions.

Example:

```ron
(
    version: 1,
    id: "material.magic_flame",
    ...
)
```

Migrations should operate on the semantic AST.

This is much safer than migrating serialized graph nodes.

---

# 29. Non-Goals for the First Version

The initial material system should **not** attempt to implement:

- every Unreal Material node;
- every Blender shader feature;
- full physically based rendering;
- arbitrary shader stages;
- arbitrary compute shaders;
- arbitrary GPU memory access;
- a perfect node graph editor;
- custom material DSL syntax;
- marketplace integration.

The first objective is a robust VFX-specific semantic material system.

---

# 30. Implementation Milestones

The implementation order matters.

Aestra should establish the semantic foundation before building a graph editor.

---

## Milestone 0 — Current Material Audit

**Status: complete.** See
[`docs/material-system/current-state.md`](material-system/current-state.md).

### Goal

Understand the current rendering/material contract before changing it.

### Tasks

- inventory current material structures;
- inventory renderer/material relationships;
- inventory existing WESL shader entry points;
- document currently supported:
  - blend modes;
  - textures;
  - particle color;
  - softness;
  - depth;
  - sprite properties;
- identify which current material properties become:
  - render state;
  - program expressions;
  - instance parameters;
- identify existing authoring commands touching materials;
- add regression fixtures for existing effects.

The audit found a fixed single-texture GPU ABI, core-owned material semantics,
and native-GPU screenshot coverage but no CPU pixel-parity reference. The later
milestones below incorporate those constraints explicitly.

### Deliverable

`docs/material-system/current-state.md`

### Completion Criterion

Existing effects have known expected output and can be used as compatibility tests.

---

## Milestone 1 — Material Core Types

### Goal

Create the canonical semantic data model.

### Tasks

Implement:

```text
MaterialProgram
MaterialInstance
MaterialDomain
MaterialRenderState
MaterialParameter
MaterialValue
MaterialValueType
MaterialInput
MaterialExpression
MaterialOutput
MaterialProgramId
MaterialParameterId
MaterialExpressionId
MaterialProgramRef
MaterialResourceLayout
```

Define:

- project/built-in program ownership and effect-local instance ownership;
- the typed expression DAG, stable references, deterministic ordering, and
  structural validation;
- the color/alpha, texture color-space, and sampler conventions;
- schema versions and normalized RON persistence;
- structural diagnostics for duplicate/missing IDs and cycles.

Initially support only:

```text
Float
Vec2
Vec3
Vec4
Color
Texture2D
```

Initial expressions:

```text
Constant
Input
Parameter
Add
Subtract
Multiply
Divide
Lerp
Clamp
SampleTexture
```

Initial outputs:

```text
Color
Alpha
```

### Deliverable

`aestra_core::material`, integrated with the existing `EffectAsset` and
renderer `MaterialId` model without a new crate cycle.

### Tests

- expression-DAG construction;
- stable serialization;
- round-trip RON;
- invalid expression detection;
- stable-ID preservation and effect-local remapping;
- color/resource metadata round trips;
- shared-subexpression and cycle fixtures.

### Completion Criterion

A simple sprite material can be represented entirely without GPU-specific code.

---

## Milestone 2 — Material Validation and Baseline Commands

### Goal

Catch authoring errors before shader generation and establish the only supported
mutation path before migration or UI work.

### Tasks

Implement:

- type inference;
- socket/input validation;
- missing parameter detection;
- missing outputs;
- cycle detection;
- unreachable expression detection;
- domain capability checks;
- evaluation-domain checks;
- resource-declaration checks;
- render-state policy checks.

Implement transactional commands in `aestra-authoring`:

```text
Add / Remove / Replace MaterialProgram
Add / Remove / Replace MaterialInstance
SetMaterialInstanceParameter
SetMaterialInstanceRenderState
AssignRendererMaterial
SetMaterialOutput
Add / Remove / Replace MaterialExpression
RewireMaterialExpressionInput
```

Commands must validate, produce semantic diffs, and support undo/redo. Direct
mutation outside deserialization and tightly scoped compiler construction is not
an accepted authoring path.

Example:

```text
Material output Alpha expects Float but received Vec3.
```

### Completion Criterion

Invalid materials produce deterministic semantic diagnostics, and every
baseline material edit can be performed transactionally.

---

## Milestone 3 — Material IR

### Goal

Separate authoring semantics from shader backend generation.

### Tasks

Create typed compiler IR.

Implement lowering:

```text
Material AST
    ↓
Material IR
```

Implement initial optimizations:

- constant folding;
- dead expression elimination;
- trivial multiply/add simplification.

### Completion Criterion

The compiler can lower valid materials into backend-neutral typed IR.

---

## Milestone 4 — WESL Backend

### Goal

Compile semantic materials into GPU shaders.

### Tasks

Implement:

```text
Material IR
    ↓
WESL generation
```

Support:

- constants;
- particle color;
- UV0;
- effect time;
- texture sampling;
- arithmetic;
- color;
- alpha.

Generate:

- shader code;
- binding metadata;
- parameter layout;
- required inputs;
- deterministic multi-texture/sampler `MaterialResourceLayout`;
- generated binding assignments;
- backend-capability validation;
- program fingerprints and pipeline cache keys;
- visible missing-resource fallbacks.

### Completion Criterion

A semantic two-texture Aestra material renders a sprite through the existing
wgpu/Bevy backend, while ordinary instance-value changes do not rebuild shaders
or pipelines.

---

## Milestone 5 — Migrate Existing Materials

### Goal

Prove the architecture against real Aestra effects.

### Tasks

- migrate current material assets;
- convert existing material properties to `MaterialInstance` parameters;
- perform migration through Milestone 2 semantic commands;
- preserve visual output;
- compare deterministic screenshots/contact sheets;
- remove duplicated legacy material logic where safe.

### Completion Criterion

Existing demo effects render equivalently using the new material compiler in
the native GPU reference path. This is visual regression approval, not a claim
of CPU/GPU pixel parity.

This milestone should occur **before** sophisticated editor UI work.

---

## Milestone 6 — Reflection and Parameter Binding

### Goal

Expose material parameters cleanly to the rest of Aestra.

### Tasks

Implement material reflection:

```text
parameters
textures
particle attributes
scene inputs
render state
```

Support bindings:

```text
Constant
EffectParameter
EmitterParameter
```

Particle color, age, and other per-particle data remain reflected semantic
inputs in the program DAG rather than instance-parameter bindings.

### Completion Criterion

The editor can automatically generate parameter controls from a material program.

---

## Milestone 7 — Material Inspector

### Goal

Provide useful material editing without a graph.

### Tasks

Create UI for:

```text
Surface
Textures
Colors
Scalar Parameters
Vector Parameters
Bindings
```

Support:

- live preview;
- undo/redo;
- parameter edits;
- asset picker;
- binding picker;
- validation messages;
- all edits submitted through the Milestone 2 command API.

### Completion Criterion

An artist can create and modify common simple materials entirely through the inspector.

---

## Milestone 8 — VFX Semantic Primitives

### Goal

Avoid requiring low-level graph construction for common VFX techniques.

### Implement first

```text
PanUV
RotateUV
ScaleUV
Remap
Smoothstep
Fresnel
RadialMask
Dissolve
DissolveEdge
DepthFade
SoftParticle
Flipbook
```

Implementation status (2026-09-02): `PanUV`, `RotateUV`, `ScaleUV`, `Remap`, `Smoothstep`,
`RadialMask`, `Dissolve`, `DissolveEdge`, `DepthFade`, and `SoftParticle` are completed
vertical slices. They retain semantic UV/speed/time, UV/center/radians, UV/center/scale, and
value/input-range/output-range, and edge-minimum/edge-maximum/value inputs through the authored
model, validation, command history, backend-neutral IR, source mapping, and portable shader
generation. Remap extrapolates, promotes scalar bounds to a shared vector type, and maps degenerate
range components to the output minimum. Smoothstep promotes scalar edges, supports reversed edges,
and maps equal-edge components to a deterministic step instead of backend-undefined behavior.
RadialMask preserves UV, center, radius, softness, and invert sockets, clamps negative radius and
softness to zero, and resolves zero softness as a deterministic hard boundary.
Dissolve preserves source, threshold, edge-width, and invert sockets, clamps negative edge width to
zero, and resolves zero edge width as a deterministic hard cut.
DissolveEdge reuses those sockets to emit a one-sided band that peaks at the threshold and fades
across the edge width. Inversion moves the band to the opposite side, negative edge width clamps to
zero, and zero edge width produces no edge.
DepthFade retains explicit scene-depth, pixel-depth, fade-distance, and invert sockets. Both depths
are linear view-space distances in the same units as the authored distance. Negative distances
clamp to zero and zero distance becomes a deterministic hard intersection test. The portable shader
ABI reserves bind group 3 for renderer-owned view inputs; the Bevy adapter binds its separate 3D
depth-prepass texture and view reconstruction data, including an MSAA shader/layout variant. It
does not sample the active depth attachment and does not claim scene-depth support for 2D views.
SoftParticle retains explicit source-alpha, scene-depth, pixel-depth, fade-distance, and invert
sockets. It multiplies source alpha by the same deterministic depth-fade result, so it inherits the
renderer-owned prepass, MSAA specialization, and non-positive-distance behavior without introducing
a second depth contract. Flipbook completes the milestone as a renderer/material integration rather
than an expression node: the renderer owns timing, playback, random start, and the frame table, then
passes the selected atlas coordinates through the typed `Uv0` material input. This keeps one
animation authority while allowing every semantic texture program to work unchanged for sprites
and flipbooks.

Then:

```text
Voronoi
Noise
FlowMap
PolarCoordinates
Twirl
HeatDistortion
```

### Completion Criterion

The majority of sprite VFX materials can be authored using semantic operations.

---

## Milestone 9 — Material Stack Editor

### Goal

Make common material composition faster than graph editing.

### Tasks

Implement:

- ordered modifier list;
- add/remove operation (implemented for compiler-approved linear-stack edges, including safe
  reconnection and owned-helper cleanup);
- reorder when valid (implemented for direct homogeneous chains with atomic project persistence,
  external-change rejection, consumer refresh, and shared chronological undo/redo);
- per-modifier inspector (implemented for compiler-reflected literal numeric, vector, and boolean
  settings, with validated atomic project persistence and shared undo/redo);
- enable/disable (implemented as a persisted, lossless typed bypass that retains operation IDs and
  settings);
- preset insertion (implemented with compiler-owned UV Drift, Soft Dissolve, and Contrast Shape
  chains, compatible-edge discovery, useful defaults, and one atomic replacement);
- automatic lowering to AST (implemented by the stack and preset planners, which emit validated
  `MaterialProgram` replacements rather than persisting a second stack model).

Example:

```text
Base Texture
UV Pan
Noise Distortion
Dissolve
Fresnel
Soft Particle
```

### Completion Criterion

Artists can create a moderately complex flame/shield/dissolve material without opening the graph.

---

## Milestone 10 — Advanced Semantic Commands and Tool API

**Status: complete.**

### Goal

Build ergonomic high-level operations on the baseline transactional commands
from Milestone 2.

### Tasks

Add authoring commands:

```text
ReplaceMaterialExpression
WrapMaterialExpression
ConnectMaterialExpression
BindMaterialParameter
ApplyMaterialPreset
InsertMaterialOperation
```

Commands must support:

- validation;
- transactions;
- undo/redo;
- useful diffs.

Current implementation: `ApplyMaterialPreset` is the first end-to-end tool command. The authoring
planner accepts a document and semantic request, delegates graph-safe construction to the material
compiler, validates the resulting baseline transaction against the complete authoring document,
and returns both that transaction and its semantic diff without mutating storage. The editor uses
this same plan before persisting the replacement through its shared material history.
`InsertMaterialOperation` uses the same boundary and represents placement as `Start`, `End`,
`Before(expression)`, or `After(expression)`. Missing anchors fail atomically, preventing delayed
editor or tool requests from applying to a different stack position after the program changes.
`ConnectMaterialExpression` provides one stable command for expression input sockets and program
outputs. It distinguishes stale source/destination identities from invalid sockets, while complete
document validation rejects incompatible value types and evaluation domains atomically.
`WrapMaterialExpression` composes compiler-approved operation construction and connection changes
around one exact destination. The planner accepts the result only when the new operation consumes
the previous source and becomes that destination's new source; fan-out and ambiguous graph edits
remain explicit failures rather than silently changing additional consumers.
`ReplaceMaterialExpression` accepts a stable expression identity and a replacement semantic kind.
The replacement keeps that identity, and therefore every downstream connection, while its incoming
references and complete graph type/domain compatibility are validated before the planner returns a
single undoable baseline transaction and expression-specific diff.
`BindMaterialParameter` exposes program default, constant, effect parameter, emitter parameter, and
random range as explicit serializable sources rather than encoding default as a nullable value. It
resolves the instance's program and stable parameter identity, limits external bindings to exposed
effect parameters, and validates type and evaluation-domain compatibility before returning one
undoable baseline transaction with a parameter-specific semantic diff.
`ExtractMaterialFunction` is intentionally deferred to Milestone 15, where it can create the
canonical typed function/input/output/call model instead of a short-lived extraction format.

### Completion Criterion

AI and tools can perform common multi-step material transformations as one
validated transaction without directly mutating storage.

---

## Milestone 11 — AI Material API

### Goal

Provide an AI-oriented semantic interface.

### Tasks

Expose:

- inspect program;
- inspect instance;
- list parameters;
- list supported operations;
- insert operation;
- change parameter;
- replace expression;
- compile;
- return diagnostics.

Prefer structured semantic operations rather than raw source editing.

Current implementation: `MaterialInspector` accepts a stable program or material-instance target
and returns one deterministic serializable report. The report includes the authored program and
optional instance, reflected controls, stack projection, compiler-approved modifier and preset
insertion edges expressed as stable placements, and structured target diagnostics. Invalid targets
retain their authored snapshots and diagnostics while compiler-derived fields that would be
misleading are omitted. `MaterialCompilationReporter` accepts the same targets and returns the
optimized backend-neutral `MaterialIrProgram`, including expression source maps and optimization
statistics, only after both program and optional instance bindings validate. Invalid targets return
their scoped diagnostics with no partial IR. `MaterialApi` exposes inspection, non-mutating semantic
edit planning, and compilation as tagged serializable requests and responses. Failures are response
values with stable codes, human-readable context, and structured validation diagnostics, allowing a
tool client to inspect, plan, preview on a clone, and compile without direct storage mutation.
`AddFresnelEdge` completes the first semantic vertical slice without exposing expression IDs: it
adds a typed view-dependent Fresnel mask, a configurable edge color and power, and either constant
or particle-normalized-age intensity as one validated transaction. Sprite rendering derives a
stable billboard sphere normal from quad coordinates, transports normalized particle age through
the portable material varying ABI, and lowers Fresnel to backend-neutral IR and generated WESL.
The compiler reflects Fresnel as a stack operation with an editable power setting when it belongs
to a single modifier chain; independent color and alpha chains continue to use the safe Advanced
projection rather than implying a false order.

### Completion Criterion

An AI can complete tasks such as:

> Add a Fresnel edge whose intensity increases with particle age.

without manipulating node IDs or shader source.

---

## Milestone 12 — Node Graph Projection

### Goal

Provide advanced visual editing and debugging.

### Tasks

Build AST → graph projection.

Support:

- generated nodes;
- generated links;
- data types;
- output nodes;
- parameter nodes;
- input nodes;
- function nodes;
- selection synchronization with inspector.

Initially, graph editing can be limited.

### Completion Criterion

Any supported material AST can be visualized accurately as a graph.

---

## Milestone 13 — Editable Node Graph

### Goal

Allow experts to edit through the graph.

### Tasks

Implement:

- create node;
- delete node;
- connect sockets;
- disconnect sockets;
- duplicate;
- convert parameter;
- graph validation;
- auto-layout.

Every graph operation must translate to a semantic authoring command.

### Completion Criterion

The graph is a full projectional editor over the same AST.

---

## Milestone 14 — Graph Layout Metadata

### Goal

Make graphs pleasant for humans without polluting shader semantics.

### Tasks

Store separately:

```text
node position
comment boxes
collapsed state
graph zoom
manual routing hints
```

Implement:

- preserve positions after semantic edits;
- auto-place new AI-generated nodes;
- optional full auto-layout.

### Completion Criterion

AI can modify a material without destroying an artist's graph organization.

---

## Milestone 15 — Material Functions

### Goal

Enable reuse of complex semantic logic.

### Implementation status

The reusable authoring slice is implemented. `MaterialFunction` assets have stable function, input,
output, and expression identities; typed signatures; normalized RON persistence; and project-local
index resolution. `FunctionInput` and `FunctionCall` participate in the shared semantic expression
language. The compiler validates exact signatures, missing references, output types, and recursive
function graphs, then deterministically inlines calls while preserving authored call aliases in the
IR source map. Effect-project compilation stores the expanded call-free programs consumed by the
runtime and render backends. A canonical built-in catalog and project-local functions are available
in the categorized graph browser; created call nodes derive their named, typed ports from the
function signature and preserve stable socket identities through reconnect, delete, and undo.
Connected selections can now be extracted through the validated `ExtractMaterialFunction` tool.
It infers typed boundary inputs and outputs, absorbs inline implementation constants, rejects
disconnected or output-less selections, creates a normalized project-local function asset, and
rewrites external consumers to stable function calls. The editor exposes the operation in the node
context menu and with `Ctrl+Shift+E`; persistence, compilation, and asset creation are rollback-safe
and participate in the shared material undo/redo history as one edit.

### Tasks

Implement:

```text
MaterialFunction
FunctionInput
FunctionOutput
FunctionCall
ExtractMaterialFunction
```

Support:

- built-ins;
- project-local functions;
- asset references;
- compiler inlining or function emission;
- function-level validation.

### Completion Criterion

A dissolve, hologram, magic noise, or shield interference algorithm can be reused across many material programs.

---

## Milestone 16 — Material Preset Library

### Goal

Create reusable high-level building blocks for artists and AI.

### Implementation status

The catalog and project-asset slices are implemented. Presets have stable semantic identities and
portable, schema-versioned descriptors with categories, descriptions, normalized search tags,
ordered semantic modifier recipes, and editable defaults. Project-local
`.aestra.material-preset.ron` assets are indexed beside programs and functions with validation,
duplicate-ID diagnostics, stable reload checks, and deterministic merging with built-ins. UV Drift,
Soft Dissolve, Contrast Shape, and Dissolve are built in, while project-local graph recipes provide
Additive Flame, Soft Smoke, Energy Beam, Magic Shield, Hologram, Ghost, Portal, and Impact Flash.
The complete path is covered through compatibility discovery, transactional insertion, undo/redo,
machine-readable inspection/planning, deterministic cached previews, portable shader compilation,
and categorized editor presentation.

### Initial presets

```text
Additive Flame
Soft Smoke
Energy Beam
Dissolve
Magic Shield
Hologram
Heat Distortion
Ghost
Portal
Trail
Impact Flash
```

The portable sprite pack ships every entry above except the two domain-dependent looks: Heat
Distortion is gated on an explicit scene-color/refraction contract, and Trail belongs to the mesh
and ribbon material domains. Neither is represented by a misleading sprite-only approximation.

Presets should compose AST operations rather than copy opaque generated shader files.

### Completion Criterion

Common material looks can be created from a small number of high-level actions.

---

## Milestone 17 — Custom WESL Functions

### Goal

Provide an expert escape hatch without abandoning semantic authoring.

### Implementation status

Complete. A normal `.aestra.material-function.ron` asset can select either the semantic expression
body or a `custom_wesl` implementation. Typed inputs and outputs remain the public contract, while
the custom implementation declares its evaluation domain, source, and a stable output-to-entry-
point map. This keeps project indexing, function references, graph insertion, transactions, and
compiler inspection identical for both implementation kinds.

The initial sandbox deliberately accepts only ordinary function declarations. It rejects imports,
resource bindings, address-space globals, and vertex/fragment/compute entry points; validates that
every output maps to one declared function; and reserves helper-function composition for a future
schema revision. Compiler expansion preserves the typed call as semantic IR, namespaces every
declared symbol by the stable function ID, records source-line ownership for diagnostics, and lets
the normal portable shader validation reject invalid WESL signatures or bodies. The bundled Pulse
Wave function exercises the complete project asset → graph call → undo/redo → compiler → GPU path.

### Tasks

Implement typed custom function assets:

```text
name
inputs
outputs
WESL source
```

Add:

- validation;
- compilation;
- diagnostics mapping;
- security/sandbox restrictions if needed;
- function-call AST node.

### Completion Criterion

Experts can implement unsupported shader algorithms while retaining Aestra parameter reflection and composition.

---

## Milestone 18 — AI Preview Feedback Loop

### Implementation Status

The deterministic preview feedback contract is implemented. The standalone
viewer accepts evenly spaced samples, explicit 60 Hz frame indices, or explicit times resolved to
unique simulation frames. Every capture writes numbered PNGs, a contact sheet, the existing human
manifest, and a versioned JSON report suitable for tools. That report includes exact frame/time
pairs, compiler diagnostics and optimization counts, semantic material fingerprints, backend and
compatibility decisions, adapter budgets, and measured/estimated runtime metrics with provenance.
Preparation and capture failures return a non-zero status and write the same report envelope when
an output directory is known. Visual comparisons add their thresholds, per-frame RMSE, changed
fraction, coverage, centroid drift, worst-frame summary, and diff-image paths to that envelope even
when a threshold fails. AI callers can therefore apply semantic commands, render a deterministic
candidate, inspect images and quantitative deltas, and refine without coupling a model provider to
the renderer.

### Goal

Let AI evaluate its own visual modifications.

### Pipeline

```text
AI edit
  ↓
compile
  ↓
deterministic preview
  ↓
capture frames/contact sheet
  ↓
visual analysis
  ↓
refinement
```

### Tasks

Expose to AI:

- preview render command;
- exact-frame capture;
- contact sheet generation;
- compiler diagnostics;
- material/effect metrics.

### Completion Criterion

AI can iteratively improve a material based on rendered results instead of relying only on semantic assumptions.

---

## Milestone 19 — Advanced Compiler Optimization

### Goal

Keep generated shaders efficient as material complexity increases.

### Add

- common subexpression elimination;
- static branch removal;
- parameter specialization;
- texture sample analysis;
- feature pruning;
- function deduplication;
- varying minimization;
- required particle attribute pruning.

Current implementation status: the first slice is complete. Backend-neutral material IR now
merges common pure expressions deterministically, including operand-order canonicalization for
Add and Multiply, without losing the many-to-one semantic source map. Implicit-derivative texture
samples now carry an explicit IR sampling contract and are commoned only when both the texture and
UV operands match; distinct operands remain separate, and custom WESL calls remain unmerged until
their purity contracts are explicit. General optimization counts plus authored, eliminated, and
live texture-sample counts survive artifact round trips, editor inspection, and machine-readable
preview reports. Shader-static parameter reads are now specialized to their typed
defaults during IR lowering rather than only during backend emission. This enables dependent
constant folding and CSE while retaining authored parameter reflection and deterministic shader
fingerprint invalidation. The semantic graph and portable shader pipeline now also support
`Select`. When its condition folds to a shader-static Boolean, only the selected branch is lowered;
unused dynamic inputs, parameter resources, texture samples, and custom calls are absent from the
live shader layout while authored parameter metadata remains inspectable. Dynamic conditions emit
the portable shader `select` operation. Branch- and feature-pruning counts survive artifacts and
appear in editor and machine-readable reports. Explicit-LOD sampling is now available through a
typed `Sample Texture Level` graph node. Its declared texture, `Vec2` UV, and Float level lower to
an explicit IR sampling contract; the level operand participates in CSE identity, survives
artifact serialization, and emits portable `textureSampleLevel`. Gradient sampling, function
deduplication, varying minimization, and required particle-attribute pruning remain planned.

### Completion Criterion

Generated materials have performance reasonably comparable to hand-authored equivalent shaders.

---

## Milestone 20 — Mesh and Ribbon Material Domains

### Goal

Extend the architecture beyond sprites.

### Add inputs

```text
Normal
Tangent
WorldPosition
VertexPosition
RibbonUV
RibbonDirection
```

### Add outputs

```text
VertexOffset
NormalModification
```

Potential later outputs:

```text
Roughness
Metallic
Emission
```

only if Aestra requires lit/PBR VFX.

### Completion Criterion

The same semantic material architecture works for sprite, mesh, and ribbon renderers.

---

# 31. Recommended Implementation Sequence

The milestones naturally form six phases.

## Phase A — Semantic Foundation

```text
0  Current audit (complete)
1  Material core types
2  Validation + baseline transactional commands
3  Material IR
4  WESL backend
5  Existing-material migration
```

Do not build the graph before completing this phase.

---

## Phase B — Useful Artist Workflow

```text
6  Reflection/bindings
7  Inspector
8  VFX primitives
9  Material stack
```

At this point Aestra already has a useful professional material editor without a node graph.

---

## Phase C — AI-First Authoring

```text
10 Advanced semantic commands/tool API
11 AI material API
```

The semantic command layer should be stable before integrating AI deeply.

---

## Phase D — Advanced Human Authoring

```text
12 Graph projection
13 Editable graph
14 Graph layout metadata
```

The graph is implemented after the semantic representation has proven itself.

---

## Phase E — Reuse and Extensibility

```text
15 Material functions
16 Presets
17 Custom WESL
```

---

## Phase F — Next-Generation Tooling

```text
18 AI visual feedback
19 Compiler optimization
20 Mesh/ribbon domains
```

---

# 32. Recommended First Vertical Slice

Before building the entire system, implement one complete end-to-end material.

Target:

> Animated additive flame sprite.

Required features:

```text
Material Program
Material Instance

Inputs:
  UV0
  ParticleColor
  ParticleOpacity
  EffectTime

Parameters:
  main_texture
  noise_texture
  tint
  distortion
  intensity

Operations:
  SampleTexture
  PanUV
  Multiply
  Add
  Lerp

Outputs:
  Color
  Alpha
```

Workflow:

```text
RON material
   ↓
AST
   ↓
validation
   ↓
IR
   ↓
WESL
   ↓
wgpu
   ↓
Aestra preview
```

Then expose those parameters in the inspector.

The two texture parameters are deliberate: this slice must replace the current
fixed one-texture/sampler ABI with a compiler-emitted deterministic resource
layout rather than hiding that architectural boundary.

Acceptance requires:

- stable program, parameter, expression, material-instance, and texture IDs;
- normalized RON round-trip with no semantic drift;
- all construction and edits through baseline semantic commands;
- deterministic two-texture/sampler layout and backend-limit validation;
- native GPU reference screenshots/contact-sheet approval;
- artifact round-trip of the compiled program, instance DTO, and resource
  layout;
- no dependency on a node graph or graph-layout metadata;
- changing `tint`, `distortion`, `intensity`, or texture assets without shader
  recompilation, and changing render state through a pipeline cache key.

This vertical slice validates almost every important architectural boundary
without requiring a graph editor.

---

# 33. Architectural Rules to Keep

These should become explicit project rules.

## Rule 1

**The typed semantic expression DAG is the source of truth.**

Never make graph serialization authoritative.

## Rule 2

**Graph layout is editor metadata.**

Node positions must never influence shader semantics.

## Rule 3

**All edits are semantic commands.**

UI and AI should use the same authoring API.

## Rule 4

**Material Program != Material Instance.**

Do not recompile shaders for ordinary parameter changes.

## Rule 5

**High-level VFX concepts are first-class.**

Prefer `Dissolve` to forcing users and AI to rebuild dissolve math every time.

## Rule 6

**Compiler owns GPU details.**

Authoring assets should not know about bind group indices, buffer offsets, or generated shader symbols.

## Rule 7

**Typed semantics are canonical; normalized RON is initial persistence.**

A future DSL is a projection/import format. It must parse into, validate, and
round-trip through the same typed model without hidden semantic state.

## Rule 8

**Custom shader code is an escape hatch.**

Do not force every advanced use case into the semantic standard library.

## Rule 9

**Every compiler diagnostic should map back to semantic material concepts.**

Generated WESL should remain an implementation detail whenever possible.

## Rule 10

**AI must be able to inspect before modifying.**

Material state, inputs, parameters, functions, and diagnostics should be machine-readable.

---

# 34. Long-Term Vision

Aestra can eventually offer four equivalent material authoring surfaces:

```text
                MATERIAL PROGRAM

     Inspector      Stack      Graph      Code
         │            │          │          │
         └────────────┴────┬─────┴──────────┘
                           │
                     Semantic AST
                           │
                    Material Compiler
                           │
                          GPU
```

A beginner can use the inspector.

A VFX artist can use the stack.

A technical artist can use the graph.

A shader expert can use code/custom WESL.

An AI can use semantic commands.

All of them edit the same material.

That is the key difference between Aestra and a conventional shader editor:

> **Aestra should treat the material as a semantic program with multiple projections, not as a graph that happens to compile into a shader.**

This gives Aestra a material system that is simultaneously:

- artist-friendly;
- AI-friendly;
- version-control friendly;
- compiler-friendly;
- reusable;
- optimizable;
- portable;
- extensible;
- suitable for next-generation VFX authoring.
