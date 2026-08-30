# Aestra — Timeline-Centric Choreography & Reusable VFX Library UX Plan

## Purpose

This document defines the next ergonomic direction for the Aestra VFX editor.

The audited, commit-sized execution plan for the editor-only foundation is maintained in
[`aestra_ui_pre_m6_implementation_plan.md`](aestra_ui_pre_m6_implementation_plan.md).

The current editor already has the right major ingredients:

- a live 3D viewport,
- an emitter/module inspector,
- a bottom timeline,
- an asset/project browser,
- curves/profiler/change panels,
- stateless/direct-seek preview information.

However, the current UI still behaves mainly like a **particle/emitter editor with a timeline attached**.

For spell VFX and complex real-time effects, Aestra should instead evolve toward:

> **DAW-style arrangement/choreography + Niagara-style emitter/module editing + live VFX viewport + reusable effect composition.**

The timeline should become the primary place where users understand **how an effect unfolds over time**.

The module stack should remain the place where users understand **how one emitter behaves**.

The library should become the place where users **reuse and compose existing effects, emitters, materials, sprites, meshes, and flipbooks**.

The viewport should remain the place where users **see and directly manipulate the result**.

---

# 0. Delivery Boundary

This direction spans two different kinds of work and they should not be delivered as
one undifferentiated redesign.

## Pre-M6 — UX foundation

Improve the current editor without requiring new effect-composition runtime semantics:

```text
separate project Library content from the current document
make the timeline the default choreography surface
turn timeline track headers into the current-document hierarchy
improve search, selection, navigation, context actions, and empty/error states
preserve user-resized layouts and the existing emitter/module workflow
```

## M6 — Reusable effect composition

Add semantic and runtime capabilities that require format, compiler, dependency, and
execution work:

```text
project asset identity and resolution
EffectClip references
child-effect compilation and execution
instance overrides
cycle detection
nested timing and event routing
```

The pre-M6 work should prepare the interaction model for M6, but it must not introduce
UI-only placeholders for semantic features that cannot yet load, compile, save, and run.

---

# 1. Core UX Principle

Aestra should clearly separate four complementary responsibilities:

```text
Timeline / Choreography
    = WHEN things happen

Emitter / Module Stack
    = HOW particles and emitters behave

Library
    = WHAT reusable building blocks already exist

Viewport
    = WHAT the result looks like in context
```

This distinction should guide all major editor decisions.

---

# 2. Why the Timeline Should Become More Central

Spell VFX are usually thought about as sequences of visual phases, not as lists of particle emitters.

Typical mental model:

```text
Charge
    |
    v
Cast
    |
    v
Projectile Travel
    |
    v
Impact
    |
    v
Afterglow
```

A user composing a spell is more likely to think:

> "The trail starts when the projectile launches, the impact burst happens on collision, and the smoke persists afterward."

than:

> "I need to add four independent particle emitters."

The choreography surface should therefore become a primary authoring workspace.

---

# 3. Timeline as the Choreography Surface

The timeline should not be treated as a small utility/debug panel.

It should become the visual structure of the complete effect.

Example:

```text
0.00      0.30      0.70          1.30              2.80
 |         |         |              |                  |
 | CHARGE  | RELEASE |    TRAVEL    |      IMPACT      |
 |         |         |              |                  |
 | ███████ |         |              |                  | Core
 |   █████████████████████████      |                  | Aura
 |         █████████████████████████|                  | Trail
 |                               ███|███               | Flash
 |                                  ████████           | Shards
 |                                    ████████████████ | Smoke
```

The timeline answers:

> **How does this complete effect happen?**

The module stack answers:

> **How does this particular emitter work?**

These are different authoring levels and should remain visually distinct.

---

# 4. Default Choreography Workspace Layout

The default workspace should give more vertical space to the timeline.

Target structure:

```text
+-----------------------------------------------------------------------+
| Toolbar / breadcrumb / compile status                                 |
+-------------+-------------------------------------------+-------------+
|             |                                           |             |
| LIBRARY     |                                           | INSPECTOR   |
|             |                 VIEWPORT                  |             |
|             |                                           |             |
|             |                                           |             |
|             |                                           |             |
+-------------+-------------------------------------------+-------------+
|                                                                       |
|                         CHOREOGRAPHY                                   |
|                                                                       |
| Track controls |       0      .5      1      1.5      2              |
| -------------------------------------------------------------------- |
| Core           | ███████████████████████████████████                  |
| Trail          |       █████████████████████████                      |
| Impact         |                              █████                   |
| Smoke          |                                  █████████████      |
|                                                                       |
+-----------------------------------------------------------------------+
```

Suggested default proportions for the **Choreography** workspace:

```text
Viewport area:
~45–50% of central height

Timeline:
~35–40%

Remaining:
toolbar, transport, status
```

All major regions should remain resizable.

The editor should remember user-adjusted panel sizes. These proportions are a useful
first-run default, not a permanent constraint for emitter, material, or profiling work.

---

# 5. Do Not Copy a DAW Literally

Aestra should borrow the **arrangement model** from tools like Ardour, but not audio-specific complexity.

Useful DAW concepts:

```text
Tracks
Clips
Trim
Move
Duplicate
Loop
Mute
Solo
Lock
Automation lanes
Markers
Groups
Playhead
Snapping
```

Concepts that should not be copied blindly:

```text
Audio buses
Mixer routing
Tempo-centric workflow
Crossfades everywhere
Audio-specific transport terminology
```

The goal is:

> **VFX arrangement editor**, not audio workstation imitation.

---

# 5.1 Project Asset Foundation

The Library cannot be only a visual wrapper around a directory scan. Before reusable
effect composition is introduced, Aestra needs an explicit project asset index and
resolver.

The foundation must define:

```text
stable project asset identity
project-relative source location
asset type
load and validation status
direct and transitive dependencies
thumbnail/cache identity
missing-reference diagnostics
```

A semantic ID identifies an authored object, but an ID alone is not sufficient to find
an external file. A reference should therefore use a resolvable project-level handle,
conceptually:

```rust
pub struct EffectAssetRef {
    pub asset: ProjectAssetId,
}
```

The project asset index owns the mapping from `ProjectAssetId` to its current source
location. Renaming or moving a file must not silently invalidate every composition that
uses it.

Professional asset lifecycle operations include:

```text
create
import / reimport
rename / move
duplicate
delete with dependency warning
reveal source
inspect usages and dependencies
refresh / file watching
repair a missing reference
invalidate and regenerate cached previews
```

The exact storage format can be decided during architecture planning, but these
behaviors must be part of the Library contract rather than added after drag-and-drop.

---

# 6. Introduce `EffectClip`

This is the most important missing composition concept.

A reusable `EffectAsset` should be placeable inside another effect as an instance on the choreography timeline.

Example library asset:

```text
Plasma Burst
├── Flash emitter
├── Sparks emitter
├── Shockwave emitter
└── Smoke emitter
```

When dropped into another effect:

```text
EffectClip
    asset: PlasmaBurst
    start: 1.25s
```

Conceptual Rust model:

```rust
pub struct EffectClip {
    pub id: EffectClipId,
    pub effect: EffectAssetRef,
    pub start: Time,
    pub source_offset: Time,
    pub playback_duration: ClipDuration,
    pub time_scale: f32,
    pub loop_mode: LoopMode,
    pub transform: Transform,
    pub parameter_overrides: ParameterOverrides,
    pub seed: SeedMode,
}
```

An `EffectClip` is a **reference/instance**, not a destructive copy.

The timing fields deliberately distinguish parent placement from the child source
window:

```text
start
    placement time in the parent effect

source_offset
    in-point inside the referenced effect

playback_duration
    visible/active window in the parent

time_scale
    mapping between parent time and child time
```

This distinction makes move, trim, loop, and time-scale operations unambiguous. Transform
bindings can be added later when a concrete binding contract exists; the first version
should use an explicit local transform.

---

# 7. Reusable Effect Composition

A complete effect should be allowed to contain:

```text
local emitters
+
referenced reusable effects
```

Example:

```text
Prism Bloom

├── local: Prism Core
├── local: Spectrum Shards
│
├── instance: Bloom Ring
├── instance: Arcane Impact
└── instance: Spectral Afterglow
```

This lets users build complex effects hierarchically.

Example composition chain:

```text
Spark Burst
      +
Shockwave
      +
Smoke
      |
      v
Heavy Impact
      +
Ground Cracks
      |
      v
Meteor Impact
      +
Meteor Trail
      |
      v
Meteor Spell
```

An Effect Asset should therefore be able to reference other Effect Assets.

Cycle detection is required.

---

# 8. Library Object Taxonomy

The long-term asset library should explicitly distinguish reusable object types.

| Library type | Meaning |
|---|---|
| **Effect** | Complete reusable VFX composition |
| **Emitter** | Reusable simulation/emitter |
| **Module** | Reusable behavior |
| **Preset** | Parameter configuration |
| **Material** | Appearance/material |
| **Mesh** | Geometry |
| **Sprite** | Texture/image |
| **Flipbook** | Animated texture |
| **Texture** | Raw texture/mask/noise |

This taxonomy should be introduced progressively. The Library must only expose a type
once Aestra supports its complete lifecycle: creation/import, editing, serialization,
dependency resolution, compilation where relevant, and deletion diagnostics.

The initial visible taxonomy should therefore focus on types that already have or are
receiving complete workflows:

```text
Effects
Textures
Meshes
Materials
Flipbooks
```

`Emitter`, `Module`, and `Preset` categories should appear when reusable extraction and
dependency packaging for those types are real features, not as empty or partially
functional placeholders.

---

# 9. Drag-and-Drop Semantics

Dragging different asset types should have predictable meaning.

```text
Effect
    -> timeline
    = create EffectClip

Emitter
    -> timeline
    = create emitter track / emitter instance

Module
    -> module stack
    = add ModuleInstance

Material
    -> renderer/material slot
    = assign material

Mesh
    -> MeshRenderer
    = assign mesh

Sprite
    -> SpriteRenderer
    = assign sprite

Flipbook
    -> FlipbookRenderer
    = assign flipbook
```

Drag targets should provide clear visual affordances and reject invalid drops.

---

# 10. The Left Panel Should Become `LIBRARY`

The current left panel mixes:

```text
CURRENT EFFECT
PROJECT EFFECTS
RENDER ASSETS
MATERIALS
FLIPBOOKS
LAYERS
```

This mixes two different concepts:

## Project/library content

```text
Effects
Emitters
Materials
Meshes
Sprites
Flipbooks
Presets
```

## Current effect/document structure

```text
Prism Core
Spectrum Shards
Floating Dust
Bloom Ring
```

These should be separated.

The left panel should primarily become:

```text
LIBRARY
```

The current effect structure should move into the choreography timeline's **track-header
tree**, which also serves as the current-document outliner. It must remain usable even
when clips are outside the visible time range or nested content is collapsed.

The track-header tree owns current-document operations such as:

```text
selection
hierarchy disclosure
reordering
context actions
enabled/preview state
diagnostic status
type/renderer indicators
```

This avoids a duplicate hierarchy panel without removing the user's stable view of the
open document.

---

# 11. Library UI Direction

Example:

```text
+ LIBRARY -------------------------+
| Search...                        |
|                                  |
| [All] [Effects] [Emitters]       |
| [Materials] [Sprites] [...]      |
|                                  |
| EFFECTS                          |
|                                  |
| +----------+  +----------+       |
| | preview  |  | preview  |       |
| |          |  |          |       |
| +----------+  +----------+       |
| Plasma Burst   Ember Sigil       |
|                                  |
| +----------+                     |
| | preview  |                     |
| |          |                     |
| +----------+                     |
| Bloom Ring                       |
+----------------------------------+
```

The Library should visibly separate ownership and origin:

```text
Project Library
Built-in Content
Current Document Resources
```

Project Library entries are reusable project assets. Current Document Resources are
embedded definitions belonging to the open effect. Moving content between those scopes
requires an explicit action such as `Make Reusable` or `Make Local`; the UI must never
imply that an embedded material is already a project-wide reusable asset.

VFX assets should have visual thumbnails.

For effects, thumbnails should ideally be animated or play on hover.

Useful library features:

```text
search
tags
folders/collections
favorites
recent
project assets
built-in assets
preview
drag/drop
```

Professional interaction also requires keyboard navigation, accessible names, list/grid
view choice, clear loading and invalid states, context menus, multi-selection where an
operation supports it, and virtualization or incremental construction for large projects.

---

# 12. Instance Parameter Overrides

Reusable effects need exposed parameters.

Example source effect:

```text
Arcane Impact

EXPOSED
------------------
Color          Violet
Radius         2.5 m
Intensity      1.0
Duration       0.65 s
Shard Count    24
Directionality 0.0
```

An instance can override only selected values:

```text
Arcane Impact instance

Color          Blue        overridden
Radius         4.2 m       overridden
Intensity      1.4         overridden
Duration       inherited
Shard Count    inherited
```

The source asset remains unchanged.

Overrides should be keyed by stable source `ParameterId`, retain their authored value
type, and be validated against the currently resolved source asset. If the source removes
or changes the type of an exposed parameter, Aestra must preserve the unresolved override
for repair and report an actionable diagnostic rather than silently discarding it.

---

# 13. Inheritance UI

The inspector should clearly distinguish:

```text
Inherited
Overridden
Default
```

Possible visual controls:

```text
Radius  [4.2]    ↺
```

where the reset action returns the instance to the source value.

The user should always understand whether they are:

```text
editing the source asset
or
editing this instance only
```

---

# 14. Three Essential Instance Actions

Every referenced effect instance should support:

```text
Edit Source
Open Instance
Make Unique
```

## Edit Source

Open the referenced reusable asset itself.

Changes update all instances using it.

## Open Instance

Edit the local instance overrides while preserving the original asset.

## Make Unique

Fork/copy the asset so this instance can diverge permanently.

These operations need to be obvious in context menus and inspector actions.

---

# 15. Nested Timeline Expansion

Referenced effects should appear as collapsible clips/tracks.

Collapsed:

```text
> Heavy Impact      █████████
```

Expanded:

```text
v Heavy Impact      █████████
    Flash           ██
    Shockwave        ████
    Shards            █████
    Smoke              █████████
```

This allows users to work at:

```text
high-level composition
or
low-level detail
```

without switching mental models.

---

# 16. Timeline Row Types

The choreography timeline should project multiple semantic object types into distinct row
presentations. A row type is initially a UI projection, not necessarily a serialized
generic `Track` object.

Initial likely types:

```text
Emitter Row
Effect Clip Row
Event Row
Parameter Automation Row
Marker Ruler
```

Possible later types:

```text
Light Track
Decal Track
Audio Event Track
Camera Event Track
Gameplay Event Track
```

Aestra does not necessarily execute all external behaviors itself.

It can emit semantic events consumed by the game.

---

# 17. Semantic Timeline Markers

Do not force effects to rely only on raw timestamps.

Support named markers:

```text
CHARGE
RELEASE
IMPACT
END
```

Example:

```text
| CHARGE
|
       | RELEASE
       |
                    | IMPACT
                    |
                                   | END
```

Tracks/clips can reference markers.

Example:

```text
Trail.start
    = Marker(RELEASE)

Impact.start
    = Marker(IMPACT)

Afterglow.start
    = Marker(IMPACT) + 80ms
```

If `IMPACT` moves:

```text
1.20s -> 1.55s
```

all marker-relative content moves correctly.

This is much more robust than absolute timing.

---

# 18. Marker-Aware AI

Semantic markers are particularly useful for future AI editing.

Example user request:

> Make the shards happen 100 ms after impact.

AI should create:

```text
ShardBurst.start
    = Marker(IMPACT) + 0.1s
```

instead of calculating an arbitrary timestamp.

---

# 19. Event Track

Events should appear visually in choreography.

Example:

```text
               RELEASE                 IMPACT
                  |                       |
Events    --------◆-----------------------◆-----

Core      ███████████████████████████████████

Trail            █████████████████████████

Impact                                    ██████

CameraShake                               ◆

PointLight                                ███

Sound                                     ◆
```

Semantic events could include:

```text
OnRelease
OnImpact
PlaySound
CameraShake
GameplayNotify
SpawnChildEffect
```

Game-specific consumers can subscribe to these events.

Timeline events are distinct from the existing emitter-to-emitter particle event links.
Use separate semantic names and types so authoring, diagnostics, and runtime routing do
not conflate them:

```text
ParticleEventLink
    particle lifecycle routing such as OnSpawn / OnDeath / OnCollision

ChoreographyEvent
    a timed semantic event such as PlaySound / CameraShake / GameplayNotify

TimelineMarker
    a named temporal anchor with no event payload by itself
```

---

# 20. Timeline Automation Lanes

Borrow the useful automation-lane concept from DAWs.

Example:

```text
v Prism Core

  Spawn Rate      -----╮
                       ╰----------

  Scale           ╭------------
                --╯

  Intensity     ---╮   ╭------
                   ╰---╯
```

Inline automation should cover commonly adjusted parameters.

The dedicated `Curves` panel remains useful for detailed curve editing.

The two systems should operate on the same semantic curve data.

---

# 21. Track Header Controls

Current simple numbered names should evolve toward richer track controls.

Example:

```text
v  S M L   Prism Core          Sprite
v  S M L   Spectrum Shards     Mesh
v  S M L   Floating Dust       Sprite
>  S M L   Bloom Ring          Effect
```

Possible controls:

```text
S = Solo
M = Mute
L = Lock
```

These controls do not all represent the same kind of state:

```text
Enabled
    authored semantic state saved in the effect

Solo
    editor preview state, not runtime asset data

Lock
    editor authoring state, not runtime asset data

Mute
    must be explicitly defined as preview-only or authored disable;
    it must not ambiguously change meaning between sessions
```

The first version should reuse the existing authored `enabled` behavior and add preview
solo/lock only when their editor-state persistence and interaction semantics are defined.

Also useful:

```text
renderer type icon
effect-instance icon
GPU/CPU/stateless indicator
compile/error status
```

Avoid permanent visual overload.

Secondary controls can appear on hover.

---

# 22. Timeline Clip Operations

Users should expect standard arrangement operations:

```text
move
trim
duplicate
delete
split
loop
time-scale
snap
lock
mute
solo
group
multi-select
copy/paste
```

Not every clip type must support every operation.

For example:

```text
trim:
may alter playback window

time-scale:
may modify child-effect time

loop:
only available when semantically valid
```

---

# 23. Snapping

Support snapping to:

```text
markers
clip starts
clip ends
playhead
frame boundaries
events
other tracks
```

Current `Snap: Smart` is a good direction.

Make snapping behavior discoverable.

---

# 24. Transport Should Be Attached to Choreography

Playback controls are temporally related to the timeline and should feel connected to it.

This is a visual and interaction relationship, not a code-ownership change. The existing
`EditorTransportPlugin` should remain the single owner of playback commands and state;
the choreography UI consumes that shared contract rather than implementing a second
transport system.

Suggested transport strip:

```text
|<   <   Play   >   >|    Loop    00:01.467    60 fps
```

Include:

```text
Play / Pause
Restart
Previous frame
Next frame
Loop
Current time
Duration
Simulation FPS
Seed
Seek mode
```

---

# 25. Preserve Direct Seek / Stateless as a Feature

Current information such as:

```text
DIRECT SEEK
STATELESS
Seed ...
```

is valuable.

Instead of leaving it as low-level debug information, expose it as understandable preview behavior.

Examples:

```text
Seek: Direct
```

or:

```text
Seek: Cached Simulation
```

This can become an Aestra differentiator.

---

# 26. Workspaces

One fixed layout should not attempt to optimize every task.

Provide curated workspace presets.

## Choreography

Primary/default complete-effect authoring.

```text
large viewport
large timeline
library
inspector
```

## Emitter

Detailed emitter behavior editing.

```text
large module stack / graph
viewport
small timeline
inspector
```

## Material

```text
large material/shader graph
material preview
properties
```

## Assets

```text
large library/content browser
asset preview
metadata/import settings
```

## Profile

```text
viewport
GPU profiler
particle counts
overdraw
timeline
```

Users should still be able to resize panels.

---

# 27. Panel Maximize

Major panels should support:

```text
Maximize
Restore
```

At minimum:

```text
Viewport
Timeline
Graph
Curves
Profiler
Library
```

A technical artist often needs a temporary full-screen graph or timeline.

---

# 28. `Create Reusable Effect from Selection`

This should be a first-class workflow.

Example current composition:

```text
Prism Bloom
├── Prism Core
├── Spectrum Shards
├── Floating Dust
└── Bloom Ring
```

User selects:

```text
Spectrum Shards
+
Bloom Ring
```

Context action:

```text
Create Reusable Effect from Selection
```

Enter name:

```text
Prismatic Burst
```

Result:

```text
LIBRARY
└── Effects
    └── Prismatic Burst
```

The original selected tracks may optionally be replaced with a single:

```text
Prismatic Burst instance
```

This is analogous to:

```text
precomposition
prefab creation
subgraph extraction
nested composition
```

and is essential for scalable reuse.

---

# 29. Reusable Emitter Workflow

The same concept should apply one level lower.

User can convert a custom emitter into:

```text
EmitterAsset
```

Example:

```text
Spectral Wisps
```

Then reuse it in multiple effects.

Possible actions:

```text
Create Emitter Asset from Selection
Edit Source
Make Local Copy
```

This should follow reusable Effect composition rather than ship in its first vertical
slice. An emitter can reference effect-local parameters, materials, textures, flipbooks,
curves, and renderers, so extraction must either package those dependencies or expose a
clear dependency contract. Copying only the `Emitter` value would create fragile assets.

---

# 30. Reusable Module / Subgraph Workflow

Reusable behavior should also be extractable.

Example:

```text
Curl Noise
+
Vortex Force
+
Velocity Clamp
```

can become:

```text
Magic Swirl Module
```

This aligns with the larger Aestra architecture where technical artists can create reusable module definitions/subgraphs.

This category should remain hidden in the Library until the custom module/subgraph
language, editing workflow, compiler contract, and dependency packaging are implemented.

---

# 31. Library Reuse Hierarchy

Recommended semantic hierarchy:

```text
Module
   |
Emitter Asset
   |
Effect Asset
   |
Effect Instance / Clip
   |
Choreography
```

Library:

```text
Library
├── Effects
├── Emitters
├── Modules
├── Presets
├── Materials
├── Meshes
├── Sprites
├── Flipbooks
└── Textures
```

This hierarchy should become central to the product.

---

# 32. Library Search Metadata

Reusable assets should expose searchable metadata:

```text
name
type
tags
style tags
author
source
performance tier
renderer types
duration
looping
platform compatibility
```

Example tags:

```text
impact
arcane
violet
large
additive
mobile-safe
```

This will later support both human search and AI asset discovery.

---

# 33. Asset Preview

Effect and emitter assets need visual preview.

Possible behaviors:

```text
static thumbnail
animated thumbnail
hover-to-play
spacebar preview
double-click open source
```

For VFX, a filename alone is not sufficient for discovery.

Deliver previews progressively:

```text
1. cached static thumbnail
2. explicit preview action / spacebar preview
3. animated or hover-to-play preview
```

Animated previews should not block the initial Library redesign. They require bounded
preview scheduling, cache invalidation, deterministic capture, and resource budgeting so
large libraries do not continuously simulate off-screen effects.

---

# 34. Favorites and Collections

Useful later:

```text
Favorites
Recent
Project
Built-in
Collections
```

Example collections:

```text
Impacts
Trails
Smoke
Magic
Fire
Electricity
UI Effects
```

Do not require deep folder hierarchies for all organization.

Tags/search should remain primary.

---

# 35. Example Spell Composition Workflow

Target workflow:

```text
1. New Effect: Arcane Missile

2. Search library:
   "spectral trail"

3. Drag Spectral Trail onto timeline.

4. Search:
   "heavy arcane impact"

5. Drag Heavy Arcane Impact to IMPACT marker.

6. Search:
   "shard burst"

7. Drag Shard Burst slightly after IMPACT.

8. Override:
   Color = blue
   Radius = 4.0
   Shard count = 18

9. Add local custom emitter for unique projectile core.

10. Preview.

11. Select impact + shard composition.

12. Create Reusable Effect from Selection:
    "Blue Arcane Impact"

13. Continue refining.
```

The user should only need to open low-level module graphs when a reusable building block does not already satisfy the desired behavior.

---

# 36. Example UI Target

Conceptual target:

```text
+----------------------------------------------------------------------------+
| AESTRA   Prism Bloom                Play  Stop                 GPU 0.18 ms  |
+----------------+-----------------------------------------+-----------------+
| LIBRARY        |                                         | INSPECTOR       |
|                |                                         |                 |
| Search         |                                         | Prism Core      |
|                |                 VIEWPORT                |                 |
| Effects        |                                         | Transform       |
| Emitters       |                                         |                 |
| Materials      |                                         | Emitter Update  |
| Sprites        |                                         |   Emission      |
| Flipbooks      |                                         |                 |
|                |                                         | Particle Spawn  |
| [preview]      |                                         |   Shape         |
| Plasma Burst   |                                         |   Initialize    |
|                |                                         |                 |
+----------------+-----------------------------------------+-----------------+
| CHOREOGRAPHY                                              Snap: Smart      |
|                                                                            |
|               CHARGE       RELEASE              IMPACT                    |
|                  ◆             ◆                    ◆                      |
|                                                                            |
| v Prism Core    ██████████████████████████████████████                    |
| v Shards              ██████████████████████                              |
| > Plasma Burst                                  ████████                   |
| > Smoke                                             █████████████          |
|                                                                            |
| + Add Track        + Add Effect        + Marker        + Event             |
+----------------------------------------------------------------------------+
```

---

# 37. Inspector Behavior

Inspector contents should depend on selection.

## Select emitter track

Show:

```text
transform
timing
emission
particle spawn modules
particle update modules
renderers
```

## Select EffectClip

Show:

```text
source effect
start
source offset
playback duration
time scale
loop mode
transform
parameter overrides
seed
Edit Source
Make Unique
```

## Select marker

Show:

```text
name
time
color/optional category
references
```

## Select event

Show:

```text
event type
time/marker binding
payload
target
```

---

# 38. Breadcrumbs

Nested reusable effects need clear navigation.

Example:

```text
Prism Bloom
/
Heavy Impact
/
Shard Burst
```

The user should always know whether they are editing:

```text
source asset
nested source
instance overrides
```

---

# 39. Source vs Instance Editing Must Be Explicit

Avoid accidental global edits.

Suggested indicator:

```text
EDITING SOURCE
Heavy Impact
```

versus:

```text
EDITING INSTANCE
Heavy Impact in Prism Bloom
```

Instance overrides should never silently modify the source.

---

# 40. Cycle Detection

Because effects can reference other effects, prevent:

```text
Effect A -> Effect B -> Effect A
```

Validation should report a clear error.

Example:

```text
ERROR
Effect reference cycle detected:

Meteor Impact
-> Heavy Impact
-> Meteor Impact
```

Cycle detection belongs to the project dependency resolver and compiler boundary, not
only to validation of one isolated `EffectAsset`. The resolver must inspect transitive
references, report the complete reference chain, and enforce a bounded nesting depth
even for acyclic graphs.

---

# 41. Time Mapping for Nested Effects

An EffectClip needs a defined local-time transform.

Conceptually:

```text
child_time =
    source_offset
    + (parent_time - clip_start)
    * time_scale
```

Support:

```text
source offset / in-point
playback duration window
time scale
looping
```

Nested timelines should display transformed child timing correctly.

---

# 42. Marker Propagation

Decide how nested effect markers behave.

Recommended initial behavior:

```text
child markers remain local to the child effect
```

Optional later feature:

```text
expose selected child markers to parent
```

Example:

```text
Heavy Impact exposes:
    FLASH_PEAK
    END
```

This prevents parent timelines from being flooded by internal implementation markers.

---

# 43. Parameter Exposure

Reusable effects should explicitly choose which parameters are public.

Internal:

```text
NoiseOctaves
InternalParticleCapacity
DebugValue
```

should not necessarily be exposed.

Public interface:

```text
Color
Radius
Intensity
Duration
Directionality
```

This gives Effect Assets a clean reusable API.

---

# 44. Effect Asset as a Reusable Component

Think of each Effect Asset as having:

```text
Visual implementation
+
Timeline
+
Public parameter interface
+
Events/markers
+
Performance metadata
```

This makes Effect Assets closer to reusable visual components than loose collections of emitters.

---

# 45. AI Implications

The timeline/library architecture should be designed for AI even if AI is not implemented in this UI phase.

AI should later be able to perform operations such as:

```text
find_effects(tags = ["impact", "arcane"])

insert_effect(
    effect = HeavyArcaneImpact,
    at = Marker(IMPACT)
)

set_effect_override(
    instance = ...,
    parameter = Radius,
    value = 4.0
)

move_clip(
    clip = ShardBurst,
    start = Marker(IMPACT) + 100ms
)

create_reusable_effect_from_selection(...)
```

This is much safer and more powerful than AI manipulating raw timeline pixels.

---

# 46. Semantic Commands Required

All choreography edits should go through normal Aestra commands.

Potential commands:

```rust
pub enum ChoreographyCommand {
    AddEffectClip(AddEffectClipCommand),
    RemoveEffectClip(RemoveEffectClipCommand),
    MoveEffectClip(MoveEffectClipCommand),
    ResizeEffectClip(ResizeEffectClipCommand),
    SetEffectClipTimeScale(SetEffectClipTimeScaleCommand),

    AddMarker(AddMarkerCommand),
    MoveMarker(MoveMarkerCommand),
    RenameMarker(RenameMarkerCommand),

    AddEvent(AddEventCommand),
    MoveEvent(MoveEventCommand),

    SetInstanceOverride(SetInstanceOverrideCommand),
    ResetInstanceOverride(ResetInstanceOverrideCommand),

    CreateReusableEffect(CreateReusableEffectCommand),
    MakeEffectClipUnique(MakeEffectClipUniqueCommand),
}
```

Exact enum structure can differ.

The important requirement is:

```text
UI drag/drop
Timeline editing
AI editing
Undo/redo
```

all use the same semantic command infrastructure.

---

# 47. Semantic Timeline Model

The timeline should not be stored as arbitrary visual rows.

Do not introduce a generic semantic `Track` abstraction until Aestra needs behavior that
cannot be represented by its existing timed objects. Emitters already own semantic
`start_time` and `duration`; wrapping them in a second track model would duplicate
ownership and create synchronization problems.

The initial semantic model should remain lean:

```rust
pub struct EffectAsset {
    pub emitters: Vec<Emitter>,
    pub effect_clips: Vec<EffectClip>,
    pub markers: Vec<TimelineMarker>,
    pub choreography_events: Vec<ChoreographyEvent>,
    // existing effect data...
}
```

The editor projects those objects into timeline rows and a track-header hierarchy:

```text
Emitter -> emitter row and clip
EffectClip -> effect-instance row and clip
TimelineMarker -> marker ruler
ChoreographyEvent -> event row/ruler
```

UI row ordering, grouping, row height, and disclosure can remain editor metadata. A
semantic heterogeneous `Track` model can be introduced later if multiple clips per track,
automation ownership, or grouping semantics demonstrate that it is necessary.

---

# 48. Editor State vs Semantic State

Keep semantic choreography separate from layout preferences.

Semantic:

```text
clip start
clip source offset and playback duration
marker time
parameter automation
effect reference
authored enabled state
```

Editor-only:

```text
track row height
collapsed/expanded state
timeline zoom
horizontal scroll
selected lanes
panel sizes
solo state
authoring locks
```

This matches the broader Aestra architecture rule:

> UI state must not become the semantic model.

---

# 49. Visual Differentiation of Track/Clip Types

Use subtle but consistent visual language.

Examples:

```text
Emitter
Effect Clip
Automation
Event
Marker
```

should be visually distinguishable through:

```text
icon
shape
border/style
label
```

Do not rely only on color, because of accessibility and theme differences.

---

# 50. Current UI Elements to Preserve

The current UI already contains several strong ideas that should remain.

Preserve:

- dark technical aesthetic,
- strong viewport focus,
- compact module stack,
- explicit stage labels such as `EMITTER UPDATE`, `PARTICLE SPAWN`, `PARTICLE UPDATE`,
- compile state feedback,
- particle count/runtime information,
- timeline frame/time information,
- stateless/direct-seek information,
- bottom tabs for Curves / Profiler / Changes where appropriate,
- clean visual density compared with heavier professional editors.

This redesign should primarily change **information architecture and workflow**, not radically restyle the application.

---

# 51. Current UI Elements to Rework

## Rework left panel

From:

```text
Assets + current effect + layers
```

to:

```text
Library
```

## Rework timeline

From:

```text
secondary bottom panel
```

to:

```text
primary choreography surface
```

## Move current-effect hierarchy

From:

```text
left panel Layers section
```

to:

```text
timeline tracks / nested effect hierarchy
```

## Rework transport

Move it visually closer to the timeline.

## Add reusable effect affordances

Library effects must clearly support:

```text
drag -> timeline
```

---

# 52. Recommended Planning Order for Codex

Codex should inspect the current implementation first and produce a migration plan before making broad UI changes.

Recommended planning order:

## Phase A — Audit current architecture

Identify:

```text
where effect layers are stored
where timeline data is stored
how emitters are selected
how assets are registered
how drag/drop currently works
how undo/redo works
how UI panels share state
how effect serialization works
which resources are embedded in EffectAsset versus project-level
which format version and migration policy the architecture documents declare
```

Do not redesign data structures blindly from the screenshot.

The audit must also correct stale architecture documentation before a new format change;
the current semantic format is v3, so older references to v2 must not guide M6 planning.

## Phase B — Pre-M6 Library/current-document separation

Improve the existing information architecture without changing effect runtime semantics:

```text
rename/reframe Assets as Library
separate Project Library, Built-in Content, and Current Document Resources
add search and type filtering for real asset types
move emitter hierarchy controls into the timeline track-header tree
preserve selection, diagnostics, and existing authoring actions
```

Do not add empty Emitter, Module, or Preset categories before their complete reusable
asset workflows exist.

## Phase C — Pre-M6 choreography workspace

Make choreography a major default region using the existing timed emitters:

```text
larger resizable timeline
stable current-document track headers
clip selection and context actions
existing move/trim/snapping behavior
remembered user layout
```

This phase should not require a format change.

## Phase D — Project asset index and resolver

Define the project-level identity and lifecycle needed by reusable references:

```text
ProjectAssetId and typed EffectAssetRef
source location resolution
dependency and usage graph
rename/move behavior
missing-reference diagnostics and repair
refresh/file watching
preview cache identity
```

This is the semantic boundary between the pre-M6 UX foundation and M6 composition.

Status: the core lifecycle slice is complete. `aestra-project` owns recursive effect discovery,
persisted-`EffectId` resolution for typed `EffectAssetRef` values, deterministic source rows, and structured
missing/duplicate/invalid/unsupported/unavailable outcomes. The Library consumes this index and no
longer treats its path-derived row key as semantic effect identity. Transitive dependency resolution,
cycle reporting, missing-reference repair, and guarded Library rename/move commands are complete.
Rename updates both the authored name and source filename, while move remains inside the indexed project
root; neither operation changes the persisted effect identity, so existing clips continue to resolve.
The editor now polls the project effect tree with a two-sample debounce, refreshes Library rows after
external add/edit/move/delete operations, and recompiles referenced previews through the catalog change
boundary. Clean open sources reload automatically; dirty editor state is preserved and reported when its
source changes on disk. Usage graphs and preview-cache identity remain in Phase D.

## Phase E — Minimal reusable Effect composition

Plan:

```text
EffectAsset -> EffectClip -> EffectAsset
```

with:

```text
source offset and playback duration
cycle detection
dependency tracking
time mapping
compiler/runtime child-effect execution
deterministic seed behavior
save/load and migrations
```

Status: the engine-independent Phase E foundation is complete. Format-v3 assets can persist an
optional `EffectClip` list without invalidating existing v3 sources. Core validation covers clip
identity and local timing; `aestra-project` resolves transitive dependencies and rejects cycles;
the compiler emits a resolved project; and the reference runtime maps clip time and deterministic
seed policy while preserving nested instance provenance. Bevy/GPU presentation intentionally
remains with the Phase F integration rather than bypassing the semantic command workflow.

## Phase F — EffectClip UI

Implement:

```text
drag effect from library
drop on timeline
create EffectClip
move
delete
select
inspect
```

The first Inspector shows the resolved source, placement start, source offset, playback
duration, seed behavior, and validation state. Add trimming, looping, and time scaling
after basic placement is stable.

Status: the command layer and first Library-to-Timeline UI slice are complete. `EffectClipId` is a
first-class selection, lock, and diff target. Dragging a resolvable project effect onto the timeline
creates a referenced clip at the pointer time, clamps its initial window to the owning effect,
shows a cursor-following drag ghost and provisional timeline bar, projects a distinct effect-instance
row and clip, selects it semantically, and preserves identity through create undo/redo. Effect clips
have preview mute/solo and contextual deletion controls, expand into their resolved source emitters,
and expose both clip metadata and child emitter/module data through a read-only Inspector. Clip
bodies move directly with timeline snapping; boundary handles trim the parent window while keeping
`source_offset` coherent and respecting parent and non-looping source bounds. The transient timing
is reflected in the Inspector and commits as one undoable semantic command. Invalid, missing,
direct-self, and transitive-cycle drops are rejected without mutating the document and retain visible
feedback.

Effect clips now use the same explicit reorder-grip interaction as local emitter tracks and can be
placed before, between, or after local emitters. Reordering changes only a stable semantic
choreography order and is one undoable command; timing and source identity do not change. Each clip
also owns a backward-compatible, non-destructive instance transform. Position,
rotation, and scale are editable with the Inspector's standard scrubbable numeric controls and the
Viewport transform gizmo, serialize with the clip, compose through nested references, and transform
the whole referenced effect without modifying its source asset.

The editor preview is now project-aware: it resolves and compiles the current root together with
its transitive reusable-effect dependencies, then synchronizes one normal Bevy `EffectPlayer` for
every active instance path. Referenced clips therefore use the same native-GPU, GPU-readback, and
CPU-fallback presentation paths as root effects while parent time remains authoritative for play,
seek, restart, source offsets, trimming, looping, and deterministic seed derivation. Root-level clip
mute/solo state filters the live instance tree, transient trim timing is reflected immediately, and
an unresolved dependency is named in the viewport without suppressing the otherwise valid root
preview. Persisted preview-state semantics remain in Phase F.

## Phase G — Instance workflow and parameter overrides

Expose reusable effect parameters and allow instance overrides.

Add:

```text
override indication
reset-to-source
explicit source/instance mode
Edit Source
Make Unique
orphaned/type-changed override diagnostics
```

Status: not started. This is the next user-facing M6 feature slice after the composition release
gate is green.

## Phase H — Nested effect expansion

Support:

```text
collapsed effect clip
expanded child tracks
breadcrumbs
```

Do not require deep nesting UI in the first iteration if it substantially complicates implementation.

Status: complete for recursive read-only expansion. Nested clip and emitter rows preserve mapped
ancestor timing, overflow through synchronized scrolling, navigate directly to referenced sources,
and expose the full source path through reusable breadcrumbs. Source mutation still requires an
explicit source-navigation action and remains separate from future instance overrides.

## Phase I — Reusable extraction

Implement:

```text
Create Reusable Effect from Selection
```

Then optionally:

```text
replace selected content with instance
or
leave original + create asset
```

## Phase J — Automation and events

After basic clips are stable:

```text
markers
event tracks
automation lanes
marker-relative timing
```

Keep `ParticleEventLink`, `ChoreographyEvent`, and `TimelineMarker` as distinct semantic
types.

## Phase K — Asset previews, advanced Library organization, and workspaces

Add progressively:

```text
cached static thumbnails
explicit and animated previews
tags, favorites, recents, and collections
large-library virtualization
```

Then add curated layouts:

```text
Choreography
Emitter
Material
Assets
Profile
```

Only after primary panel responsibilities are clear.

---

# 53. MVP Scope

The work has two explicit delivery gates.

## Pre-M6 UX foundation

```text
1. Larger choreography timeline.
2. Project Library and Current Document Resources are visually separate.
3. Existing project effects are searchable and filterable.
4. Timeline track headers become the current-document hierarchy.
5. Existing emitter selection, add/duplicate/delete, move, and trim remain available.
6. Empty, loading, invalid, and unavailable states are explicit.
7. Existing panel resizing and layout persistence remain functional.
8. No unsupported reusable asset type is advertised as functional.
```

This gate should not require an effect-format change.

## M6 reusable-composition MVP

```text
1. Project assets have stable resolvable identities.
2. An Effect Asset can be dragged from the Library onto the timeline.
3. The drop creates a referenced EffectClip, not a destructive copy.
4. The compiler/runtime can resolve and execute the referenced effect.
5. EffectClip placement, selection, deletion, and inspection work.
6. EffectClip operations use semantic commands and undo/redo.
7. EffectClip serialization and migration are stable.
8. Missing references and cycles produce targeted diagnostics.
9. Existing emitter/module editing and preview behavior do not regress.
```

Do not block this MVP on:

```text
instance parameter overrides
Edit Source / Make Unique
named or marker-relative timing
full automation lanes
advanced nested expansion
complex time warping
animated previews
reusable emitter/module extraction
audio integration
AI integration
all workspace presets
```

---

# 54. MVP Acceptance Criteria

## Pre-M6 UX foundation acceptance

- [x] The default UI clearly communicates that the timeline is a primary authoring surface.
- [x] Project Library content, built-in content, and current-document resources are no longer mixed together.
- [x] The timeline track-header tree provides a stable current-document hierarchy.
- [x] Existing project effects are searchable and filterable in the Library.
- [x] Existing emitter actions remain available from the track hierarchy or its context menus.
- [x] Existing timeline move, trim, seek, zoom, pan, and snapping behavior remains functional.
- [x] Selection remains synchronized between Timeline, Inspector, Curves, Diagnostics, and Viewport.
- [x] Empty, loading, invalid, and unavailable Library states are visually explicit.
- [x] Keyboard navigation, accessible labels, and invalid-drop feedback are preserved or improved.
- [x] User-resized layouts remain persisted.
- [x] No effect-format migration is required for this gate.

## M6 reusable-composition acceptance

- [x] Reusable Effect Assets are discoverable through the project asset index.
- [x] An Effect Asset can be dragged from the Library to the timeline.
- [x] Dropping it creates a referenced EffectClip, not a destructive copy.
- [x] Moving an EffectClip changes its semantic start time.
- [x] EffectClip rows can be reordered with stable identity and undo/redo.
- [x] EffectClip operations are undoable/redoable.
- [x] EffectClip serialization is stable.
- [x] The referenced effect resolves, compiles, runs, seeks, and restarts deterministically.
- [x] An EffectClip exposes its resolved source and timing window in the Inspector.
- [x] An EffectClip exposes an editable instance transform that affects the complete referenced effect.
- [x] Missing references identify the unresolved asset and offer a repair path.
- [x] Effect reference cycles are rejected with clear diagnostics.
- [x] Moving or renaming a project asset through the Library preserves valid references.
- [x] Existing emitter/module editing remains functional.
- [x] Existing preview/runtime behavior remains functional.
- [x] Existing direct-seek/stateless behavior is not regressed.

---

# 55. Second-Stage Acceptance Criteria

After the MVP:

- [ ] Exposed effect parameters can be overridden per instance.
- [ ] Overrides can be reset to inherited source values.
- [ ] Orphaned or type-changed overrides produce actionable diagnostics.
- [ ] The user can explicitly edit the source asset.
- [ ] The user can make an instance unique.
- [ ] The user can add and move named markers.
- [x] Nested EffectClips can be expanded for read-only source inspection.
- [x] Breadcrumb navigation clearly indicates nested source editing.
- [ ] Effect markers can be referenced by child clip timing.
- [ ] Timeline events are supported.
- [ ] Automation lanes use the same semantic curves as the Curves editor.
- [ ] `Create Reusable Effect from Selection` works.
- [ ] Reusable emitter extraction works.
- [ ] Library search supports tags/types.
- [ ] Effect previews use cached static thumbnails and optionally animated preview.
- [ ] Timeline panel can be maximized.
- [ ] Curated workspaces are available.
- [ ] Track controls expose authored Enabled plus clearly defined preview Solo/Lock state; Mute is added only with explicit persistence semantics.
- [ ] Marker-relative edits remain stable when marker times change.

---

# 56. Ergonomic Success Criteria

A new user should be able to understand the following workflow without reading documentation:

```text
Library
    |
drag reusable effect
    |
Timeline
    |
move / arrange / combine
    |
Inspector
    |
override parameters
    |
Viewport
    |
preview result
```

A technical artist should additionally understand:

```text
double-click/open
    |
Emitter / Module details
```

The common workflow should not require opening low-level graphs for every effect.

---

# 57. Target User Mental Model

The intended user mental model is:

```text
I compose a spell from reusable visual phrases.

I arrange those phrases over time.

I override the parameters I care about.

I open emitters/modules only when I need deeper control.

I can turn something useful I built into another reusable building block.
```

This is the central ergonomic target.

---

# 58. Final Product Principle

Aestra should not become:

> a node graph with a small timeline and an asset list.

It should become:

> a visual-effects choreography environment where reusable effects can be arranged, nested, parameterized, inspected, and refined at multiple abstraction levels.

The four major interaction surfaces should have clear roles:

```text
Timeline
    composition / choreography

Module Stack
    behavior

Library
    reuse / discovery

Viewport
    visual feedback / direct manipulation
```

The most important next workflow to make effortless is:

```text
Library
    ->
drag reusable Effect
    ->
Timeline EffectClip
    ->
position relative to markers
    ->
override exposed parameters
    ->
preview
    ->
save useful combinations as reusable Effects
```

This should be the foundation for the next Aestra editor UX iteration.
