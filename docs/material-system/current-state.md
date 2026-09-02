# Current Material System Audit

Status: Material authoring Milestones 5 and 6 complete; Material 7 in progress
Audited: 2026-09-02

This document records the material and renderer contract that exists before Aestra introduces a
typed material program. It is the compatibility baseline for the first material vertical slice;
it is not a proposal to preserve every current type indefinitely.

## Scope and compatibility promise

The current system supports reusable sprite materials, sprite and flipbook renderer instances,
typed constant or effect-parameter inputs, engine-neutral compiler plans, and native GPU
presentation. It does not yet provide a general expression tree, a material-specific IR, custom
material WESL, lighting, mesh/ribbon material domains, or a node graph.

The first material-program implementation must preserve:

- stable material, renderer, texture, flipbook, and parameter identities;
- deterministic format-v3 loading and validation;
- existing alpha, additive, and multiply sprite output;
- particle-color and constant/parameter tint behavior;
- texture/UV and flipbook sampling behavior;
- undoable semantic editing;
- engine-neutral compiled contracts and explicit Bevy/WGPU adaptation.

## Current data flow

```text
EffectAsset
  ├─ AssetDefinition (project-relative texture path)
  ├─ FlipbookDefinition (texture + explicit UV frames)
  ├─ MaterialDefinition (shared sprite presentation state)
  └─ Emitter
       └─ RendererInstance (material ID + sprite/flipbook configuration)
              │
              ▼
EffectCompiler
  ├─ CompiledAsset / CompiledFlipbook
  ├─ CompiledMaterial
  └─ RendererPlan
              │
              ├─ CPU/readback sprite presentation
              └─ aestra-gpu packed renderer ABI
                         │
                         ▼
                Bevy render pipeline + WESL
```

Materials are shared assets. An emitter renderer references a material by `MaterialId`; it does
not own a copy. Multiple enabled renderers may be attached to one emitter and may reuse the same
material.

## Authored semantic contract

### MaterialDefinition

`aestra-core` currently defines one material domain through `MaterialProperties::Sprite`.

| Field | Current representation | Current behavior | Future classification |
| --- | --- | --- | --- |
| `id`, `name` | Stable `MaterialId`, authored label | Identity, references, navigation | Material asset identity/metadata |
| `blend` | `Alpha`, `Additive`, `Multiply` | Selects GPU blend state and fragment entry point | `MaterialRenderState` |
| `softness` | `MaterialInput<f32>` | Controls analytic edge coverage | Program input/expression with a typed default |
| `color` | Particle color or `MaterialInput<[f32; 4]>` | Multiplies particle color or supplies a tint | Program input/expression |
| `texture` | Optional stable texture `AssetId` | Samples an imported texture or uses white | Material instance resource binding |
| `uv` | Normalized `UvRect` | Selects the sampled sprite region | Material instance value |

`MaterialInput<T>` supports only `Constant(T)` and `Parameter(ParameterId)`. A bound parameter is
kept as a runtime slot only when it is public/exposed; otherwise the compiler folds its current
default into a constant expression. This is a useful compatibility seam, but not a general
material expression language.

`SpriteColorSource::ParticleColor` is an explicit semantic dependency on the simulated particle
color attribute. `SpriteColorSource::Value` supplies a constant or parameter tint instead.

### Renderer and material relationship

`RendererInstance` owns a stable `RendererId`, enabled state, open `RendererTypeId`, referenced
`MaterialId`, and renderer-specific properties.

| Renderer properties | Semantic model | Compiler/runtime | Presentation |
| --- | :---: | :---: | :---: |
| Sprite | Implemented | Implemented | CPU/readback and native GPU |
| Flipbook | Implemented | Implemented | CPU/readback and native GPU |
| Ribbon width | Representable | Rejected as unsupported | Not implemented |
| Mesh asset | Representable | Rejected as unsupported | Not implemented |
| Custom values | Representable | Rejected as unsupported | Not implemented |

Flipbook renderer state includes a stable `FlipbookDefinition` reference, particle-age or
effect-time playback, forward/reverse/ping-pong direction, and deterministic random start.
`FlipbookDefinition` owns an imported texture reference, explicit normalized frame rectangles,
frame rate, and looping policy. This animation state belongs to the renderer/asset relationship,
not to the future surface-expression tree.

### Assets and validation

Texture assets use stable IDs and non-empty project-relative paths without absolute paths or
parent traversal. Material textures must reference registered texture assets. Flipbooks require a
registered texture, at least one finite positive-area normalized frame, and a positive finite
frame rate. Material softness must be finite and non-negative, tint components must be finite,
and UV bounds must be normalized with positive area.

Referenced materials cannot be removed while a renderer uses them. Renderer type IDs must agree
with their typed properties, and the compiler rejects currently unsupported renderer domains with
structured diagnostics.

## Compiled and runtime contract

`EffectCompiler` lowers every material into an engine-neutral `CompiledMaterial` containing:

- stable source ID and name;
- `BlendMode`;
- typed constant/parameter expression for softness;
- particle-color selection or typed constant/parameter color expression;
- optional texture ID;
- normalized UV bounds.

Each `RendererPlan` retains its renderer source ID, referenced material ID, and sprite or flipbook
kind. Compiled assets resolve stable IDs to project-relative paths. No compiled material or renderer
plan contains Bevy, WGPU, shader handle, ECS entity, or editor state.

At instance time, public parameter slots resolve material softness and tint without recompiling the
effect. `aestra-gpu` packs the resolved values, blend selector, texture flag, UV bounds, renderer
kind, and bounded flipbook frame table into `GpuRenderer` records.

### CPU/reference presentation boundary

The CPU and GPU-readback presentation path currently preserves material color selection,
texture/UV sampling, flipbook frame selection, particle size/rotation/position, and visibility.
It uses Bevy sprites as a compatibility view and does **not** currently reproduce the native GPU
analytic softness mask or authored blend mode. Therefore:

- simulation semantics remain CPU/GPU conformant;
- native material appearance is approved through the GPU visual workflow;
- future material tests must not claim full CPU pixel parity until a real reference material
  evaluator exists.

## Current shader and render-pipeline contract

The engine-neutral `aestra-gpu` crate owns two shared runtime WESL modules:

| Module | Entry points | Responsibility |
| --- | --- | --- |
| `package::aestra_simulation` | `reset`, `simulate` | Particle spawn/update, curves, gradients, compaction, indirect counts |
| `package::aestra_sprite_render` | `vertex`, `fragment_alpha`, `fragment_additive`, `fragment_multiply` | Camera-facing quad expansion and sprite surface evaluation |

The sprite module also contains a diagnostic `fragment_wireframe` presentation entry point.
WESL composition produces reviewable WGSL which is parsed and validated by Naga. Portability CI
also requires the representative simulation and sprite programs to translate to SPIR-V and HLSL.

The semantic material path additionally generates one inspectable WESL/WGSL fragment module per
typed material IR program. The same portable compilation produces deterministic uniform,
multi-texture, and descriptor-shared sampler bindings; parameter and required-input reflection;
backend-limit diagnostics; stable program fingerprints; and render-state/target pipeline keys.
`aestra-bevy-render` translates the emitted resource layout into the corresponding WGPU descriptor.
Project compilation retains every referenced semantic program and instance in the engine-neutral
compiled effect. Artifact format 2 round-trips those descriptors. The live 2D and 3D sprite
pipelines automatically compile each retained program and create an emitter-specific runtime
binding for every renderer that references its instance. Each frame they refresh scoped effect and
emitter parameters plus deterministic random values, append the generated bind group, pack instance
uniforms, resolve texture IDs through the compiled effect asset table, create explicit samplers, and
specialize through the portable material pipeline key. Renderers without a semantic instance keep
using the compatibility path. Changing ordinary instance bytes or texture IDs does not select a new
shader or pipeline; the explicit manual binding API remains available as a presentation override.

The current sprite surface function:

1. selects material tint or multiplies simulated particle color;
2. selects sprite UVs or an explicit flipbook frame;
3. samples the texture when present;
4. computes radial coverage for procedural sprites or edge coverage for textured sprites;
5. returns RGB and alpha for the selected blend pipeline.

Legacy native GPU render state is fixed except for blend mode. Semantic material draws apply their
validated blend, depth-test, depth-write, and cull state:

- triangle-list camera-facing quads;
- back-face culling;
- alpha, source-alpha additive, or multiply blending;
- reverse-Z `GreaterEqual` depth test;
- depth writes disabled;
- no authored depth, cull, stencil, alpha-to-coverage, or sorting controls.

Missing texture files use a visible checkerboard fallback and surface a diagnostic instead of
silently removing the draw.

## Authoring operations and editor surfaces

The semantic command layer owns the following relevant operations:

- add, remove, or replace a material;
- add, remove, move, duplicate, enable, or disable a renderer;
- assign a material to a renderer;
- replace renderer-specific properties;
- add, remove, or replace texture and flipbook assets.

All operations execute through validated transactions and produce inverses for undo/redo. A failed
replacement restores the original document atomically.

The editor Library creates reusable sprite materials and imported flipbooks. Properties assigns
materials, selects blend mode, edits constant softness, texture and UVs, and configures flipbook
playback. Parameter-bound softness is displayed as bound rather than mutated as a constant.
Renderer/material edits ultimately submit the same semantic commands as non-UI callers.

The current UI edits a material through a selected renderer. A dedicated future material inspector
may improve ownership clarity, but it must remain a projection of semantic commands rather than a
second mutable model.

## Compatibility fixtures

The existing baseline is deliberately split between immutable semantic fixtures and manually
approved native-GPU visuals.

| Contract | Existing coverage |
| --- | --- |
| Format/validation | `aestra-core/tests/model_contract.rs`: stable texture references, flipbook frames, multiple renderers, serialization |
| Semantic editing | `aestra-authoring/tests/authoring_contract.rs`: transactional material replacement, undo, referenced-material deletion guard |
| Compiler lowering | `aestra-compiler/tests/compiler_contract.rs`: texture resolution, flipbook metadata, material parameter folding/runtime slots |
| Material IR | `aestra-compiler/tests/material_ir_contract.rs`: deterministic typed lowering, source mapping, optimization, invalid-program rejection |
| Control reflection | `aestra-compiler/tests/material_reflection_contract.rs`: typed controls, source choices, defaults/overrides, texture constraints, input requirements, and render state |
| GPU ABI | `aestra-bevy-render/src/gpu.rs` tests: multiple renderers, blend selectors, softness, tint, texture/UV, flipbook packing |
| Shader source | `aestra-gpu/tests/shader_contract.rs`: WESL/WGSL snapshots, Naga validation, SPIR-V/HLSL translation |
| Generated material shader | `aestra-gpu/tests/material_shader_contract.rs`: two-texture flame WESL, resource ABI, reflection, fingerprints, pipeline keys, and capability diagnostics |
| Runtime ingestion | `aestra-bevy/tests/v3_contract.rs`: immutable v3 and textured-effect compilation/profile contract |
| Native appearance | `aestra-viewer/tests/references/*`: eight fixed-seed reference frames for Prism Bloom, Ember Sigil, and Plasma Burst |
| GPU approval | `.github/workflows/gpu-visual.yml`: native GPU conformance, editor viewport smoke, and showcase comparisons |

The three showcase effects currently exercise distinct material paths:

- Ember Sigil: textured additive sprites with particle color;
- Plasma Burst: additive flipbook presentation;
- Prism Bloom: multiple reusable alpha/additive sprite materials.

The checked-in images define the last approved visual baseline. Local showcase edits are not part
of semantic unit-test truth and require an explicit native-GPU reapproval before their output
becomes the new baseline.

Material 5 now provides `plan_legacy_sprite_material_migration` and
`migrate_legacy_sprite_materials`. The deterministic transaction keeps each legacy definition as a
recovery/compatibility source, creates project programs and effect-local instances, and reassigns
renderers only through baseline semantic commands. Sprite textures, flipbook atlas textures,
particle/constant/parameter tint, sampled alpha, UV rectangles, blend state, and softness are
covered. Softness uses a named reflected compatibility value until coverage becomes a first-class
semantic primitive. The viewer's opt-in `--semantic-materials` path performs this migration in
memory and binds the resulting shaders without rewriting the source effect. Native-GPU comparison
now verifies pixel-identical legacy and semantic output for all three showcase sources, and the
approved images are the scheduled workflow baseline.

## Migration classification

The next material model should move current fields as follows:

| Current concept | Destination |
| --- | --- |
| Material ID/name | `MaterialProgram` or material asset identity and metadata |
| Blend mode | `MaterialRenderState` |
| Softness | Typed material input and expression feeding coverage/alpha |
| Particle color | VFX semantic input exposed to the material program |
| Constant/parameter tint | Material parameter/default plus instance override |
| Texture ID | `Texture2D` material instance binding |
| UV rectangle | Instance parameter or renderer-provided UV transform |
| Sprite/flipbook kind | `MaterialDomain::Sprite` plus renderer configuration |
| Flipbook timing and frame table | Renderer/asset animation state, outside the expression tree |
| Implicit depth/cull defaults | Explicit validated render-state defaults |

Existing `MaterialDefinition` assets may initially lower into the new types through a compatibility
adapter. A destructive format migration is not required to prove the first vertical slice.

## Known gaps

- compiled effects carry semantic programs, instances, and their dynamic source descriptors;
  presentation creates and refreshes emitter-specific contexts automatically, and Properties now
  renders the first reflection-driven material controls for selected renderers. Constant, random
  range, texture, and boolean edits share semantic validation and the editor undo/redo history.
  Reflected source menus expose only compatible constant, random-range, and typed public
  effect/emitter bindings; authored render-state controls remain;
- the initial generated backend supports UV0, particle color/opacity, effect time, arithmetic,
  interpolation, clamping, and sampled Texture2D parameters; richer inputs remain explicit errors;
- no validated custom WESL functions;
- no mesh, ribbon, trail, lit, decal, or volumetric material domains;
- no authored depth, cull, sorting, or stencil controls;
- no CPU pixel-reference evaluator for blend and softness;
- no dedicated material preview or material inspector;
- no node-graph projection.

## Entrance contract for the first vertical slice

Material Milestone 1 may begin with an additive unlit sprite domain and only `Float`, `Vec2`,
`Vec3`, `Vec4`, `Color`, and `Texture2D` values. Before editor graph work, it must prove:

1. engine-neutral material program and instance types with stable RON round trips;
2. structured type and output validation;
3. a typed IR that can express the existing additive flame surface;
4. generated, inspectable, Naga-validated WESL/WGSL;
5. runtime parameter/resource binding through the existing compiled and GPU boundaries;
6. compatibility tests for the current constant/parameter, particle-color, texture, UV, blend,
   and softness paths;
7. native-GPU approval against a dedicated deterministic flame fixture.

The node graph remains deferred. It will be a projection of the semantic program after the typed
model, compiler path, runtime binding, and preview are stable.

Items 1–7 of this entrance contract are complete. The deterministic legacy migration covers the
current sprite and flipbook showcase paths, and native-GPU approval closes the Material 5 release
gate. Material 6 is complete with its scoped dynamic-value resolver, compiled/versioned descriptor
persistence, automatic emitter-specific presentation contexts, and reusable engine-neutral control
reflection. Material 7 can now generate Properties controls from that catalog and route edits
through semantic authoring commands. Its reflected source menus can bind compatible public effect
or emitter parameters and can switch back to editable constants or random ranges without replacing
the compiled material program.
