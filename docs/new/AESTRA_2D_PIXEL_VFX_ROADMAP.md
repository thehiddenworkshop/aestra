# AESTRA 2D & PIXEL VFX ROADMAP

> **Status:** Proposed architecture and implementation roadmap
>
> **Scope:** First-class 2D VFX, pixel-art VFX, hybrid 2.5D workflows, pixel-aware rendering, sprite/flipbook authoring, lighting, baking, Grid2D simulation, and Bevy integration.
>
> **Primary engine target:** Bevy
> **Core constraint:** Aestra assets remain engine-neutral and do not encode a permanent Bevy `Core2d` / `Core3d` split.

---

## 1. Executive summary

Aestra should support 2D and pixel-art games as first-class use cases, but **must not create separate 2D and 3D effect formats**.

Recommended model:

```text
Aestra Effect
    |
    +-- Simulation
    |     +-- Particle
    |     +-- Strip
    |     +-- Grid2D
    |     +-- Grid3D
    |     +-- Custom
    |
    +-- Renderers
    |     +-- SpriteRenderer
    |     +-- FlipbookRenderer
    |     +-- MeshRenderer
    |     +-- RibbonRenderer
    |     +-- BeamRenderer
    |     +-- LightRenderer
    |
    +-- Presentation
          +-- Standard
          +-- Pixel Art
          +-- Custom
```

Project creation should offer **authoring profiles**, not capability modes:

```text
3D
2D
Pixel Art 2D
Hybrid / 2.5D
```

Profiles configure camera, grid, defaults, sorting, units, and pixel settings. They do **not** restrict renderers or simulation domains.

A Pixel Art 2D project should still be able to use 3D meshes, ribbons, lighting, Grid2D simulation, and low-resolution offscreen rendering.

The key design principle is:

> **Pixel art is a presentation model, not a lower-fidelity simulation model.**

---

## 2. State-of-the-art references

No current tool combines all of these well in one runtime-oriented, engine-neutral system:

```text
GPU simulation
+ pixel-aware presentation
+ runtime parameters/events
+ 2D lighting/materials
+ fluids
+ baking
+ cross-engine deployment
```

Important references:

| Reference | Main lesson |
|---|---|
| Pixel Composer | Procedural pixel workflows and Grid2D-style simulation |
| SpriteMancer | Artist-friendly layered VFX workflow |
| Pixel FX Designer | Palette, dithering, looping, explicit pixel constraints |
| Pixelpart | Runtime + external parameters + baking |
| Effekseer | Cross-engine runtime and bake/export |
| Juice FX | Sprite-local hit/dissolve/warp effects |
| Unity VFX Graph | GPU VFX integrated into a 2D renderer |
| Unreal Niagara | High-end simulation and flipbook baking |
| Animal Well | Modern simulation mapped to pixel-art presentation |
| Eastward | Pixel art inside a 3D-lit world |
| Dead Cells | 3D animation -> low-resolution pixel output |
| Noita | Per-pixel / Grid2D simulation |
| Never's End | 3D world rendered as stylized pixel art |

---

## 3. Core architecture

### 3.1 One effect model

Do **not** introduce:

```text
Effect2D
Effect3D

Particle2D
Particle3D
```

Maintain one canonical `EffectAsset`.

A particle can keep general spatial attributes:

```text
position          Vec3
previous_position Vec3
velocity          Vec3
acceleration      Vec3
rotation          ...
scale             ...
```

2D behavior is expressed through constraints:

```text
SpatialConstraint
    Free3D
    PlaneXY
    PlaneXZ
    PlaneYZ
    CustomPlane
```

This preserves:

- 2.5D workflows;
- Z layering;
- mesh rendering in 2D;
- cross-engine compatibility;
- future renderer changes.

---

## 4. Project authoring profiles

Suggested semantic editor setting:

```rust
pub enum AuthoringProfile {
    ThreeD,
    TwoD,
    PixelArt2D,
    Hybrid2_5D,
}
```

Example defaults:

```text
PixelArt2D
    Camera                Orthographic
    Grid                  Pixel
    Spatial Constraint    PlaneXY
    Renderer              SpriteRenderer
    Texture Filter        Nearest
    Presentation          PixelArt
```

The profile must be switchable without migrating authored effects.

---

## 5. Bevy integration strategy

Aestra should not serialize current Bevy render-pipeline distinctions.

Target:

```text
EffectAsset
    |
Aestra IR
    |
CompiledEffect
    |
Renderer Requests
    |
aestra-bevy
    |
current Bevy 2D/3D paths
    |
future unified Bevy internals
```

Acceptance rule:

> If Bevy further merges its 2D and 3D renderer internals, existing `.aestra` assets should require no migration.

---

## 6. Target asset/runtime model

```text
EffectAsset
|
+-- Parameters
+-- Events
+-- Emitters
|   |
|   +-- Simulation Domain
|   |     +-- Particle
|   |     +-- Strip
|   |     +-- Grid2D
|   |
|   +-- Stages
|   |     +-- Spawn
|   |     +-- Update
|   |     +-- Custom Simulation Stage
|   |
|   +-- Renderers
|         +-- SpriteRenderer
|         +-- FlipbookRenderer
|         +-- MeshRenderer
|         +-- RibbonRenderer
|         +-- BeamRenderer
|         +-- LightRenderer
|
+-- Presentation Profiles
|     +-- Standard
|     +-- PixelArt
|
+-- Assets
      +-- SpriteAsset
      +-- FlipbookAsset
      +-- TextureAsset
      +-- MaterialAsset
      +-- PaletteAsset
      +-- MeshAsset
```

Simulation, renderer, asset, material, and presentation remain separate concepts.

---

## 7. SpriteRenderer

Extend the existing sprite renderer with 2D-specific orientation, sizing, and pivot controls.

Suggested configuration:

```text
SpriteRenderer
    Sprite
    Material

    Orientation
        Fixed2D
        FaceCamera
        VelocityAligned
        AxisAligned

    SizeMode
        WorldUnits
        ScreenPixels
        PixelUnits

    Sorting
        None
        Layer
        Z
        ViewDepth
        YSort
        Age
        CustomKey

    Sampling
        Linear
        Nearest
```

Required functionality:

- color/size/rotation per particle;
- pivot;
- atlas UVs;
- alpha/additive/multiply/premultiplied blend;
- sorting;
- batching;
- bounds/culling;
- multiple renderers per simulation.

---

## 8. Flipbooks

Reuse effect-local `FlipbookDefinition` metadata. It references a texture and stores explicit UV frame rectangles, frame rate, and the looping flag. The renderer selects the clock, playback direction, and random start. An importer can populate those frame rectangles from a grid or a packed atlas. Project-level sharing can build on the same representation.

Frame sampling and texture sampling are independent. Keep the current held-frame playback for pixel art and use a nearest sampler on the atlas. Frame interpolation can be introduced later as a separate renderer control.

---

## 9. Pixel Art presentation profile

Introduce reusable `PixelArtProfile` data:

```text
Virtual Resolution
Pixels Per Unit
Position Snap
Size Snap
Texture Filter
Presentation FPS
Palette
Color Quantization
Dithering
Dither Space
Antialiasing
Mipmaps
```

Recommended initial defaults:

```text
Virtual Resolution    320x180
Pixels Per Unit       16
Texture Filter        Nearest
Position              Render-Snapped
Antialiasing          Off
Mipmaps               Off
Flipbook Sampling     Hold
Dithering             None
```

---

## 10. Simulation precision vs pixel presentation

Do not round simulation positions by default.

```text
Simulation:
    x = 10.236
    y = 7.813

Presentation:
    virtual_position = camera_to_virtual_viewport(position)
    snapped_position = pixel_origin + round(virtual_position - pixel_origin)
```

Expose explicit modes:

```text
SmoothSimulationSnappedRender   <- default
QuantizedPosition
QuantizedVelocity
GridSimulation
```

This keeps collisions and forces stable while rendering intentionally stepped motion.

---

## 11. Temporal quantization

Separate:

```text
Simulation Rate
Presentation Rate
Bake Rate
```

Example:

```text
Simulation      60 Hz
Presentation    12 FPS
Display         120 Hz
```

This is critical for authentic low-frame-rate pixel animation without breaking physical simulation.

---

## 12. Palette assets

Introduce:

```text
PaletteAsset
```

Possible structure:

```text
colors
ramps
semantic groups
metadata
```

Material operations:

```text
NearestPaletteColor
PaletteIndex
PaletteRamp
PaletteShift
PaletteCycle
PaletteReplace
QuantizeToPalette
```

Palettes should be reusable project assets.

---

## 13. Dithering

Initial modes:

```text
None
Bayer2x2
Bayer4x4
Bayer8x8
BlueNoise
CustomPattern
```

Support coordinate spaces:

```text
Screen
World
Object
Particle
```

World/object-space dithering helps avoid pattern swimming.

---

## 14. Pixel-aware material nodes

Recommended initial node library:

```text
PixelCoordinate
PixelSize
PixelSnap
Posterize
Quantize
Threshold
PaletteLookup
PaletteRamp
BayerDither
PatternDither
PixelNoise
CellNoise
Voronoi
PixelOutline
PixelGlow
PixelDissolve
PixelBurn
PixelScatter
PixelateUV
PixelateScreen
SpriteNormal
HeightToNormal
```

These should compile through normal Aestra material IR.

---

## 15. Procedural pixel renderer

Consider a lightweight renderer for textureless tiny effects:

```text
PixelShapeRenderer
    Pixel
    Square
    Diamond
    Cross
    Disc
    Ring
    CustomMask
```

Good use cases:

```text
embers
stars
dust
rain
snow
magic sparks
tiny debris
```

---

## 16. 2D sorting and layers

Support semantic ordering:

```text
RenderLayer
Z
YSort
CustomSortKey
```

Do not hard-code Bevy-specific sorting rules into `aestra-core`.

---

## 17. 2D and pixel lighting

Support visual models:

```text
Unlit
Continuous
PixelQuantized
PaletteQuantized
Dithered
```

Required eventual features:

- point/directional lights;
- emissive sprites;
- 2D normal maps;
- stepped falloff;
- hard shadow masks;
- palette-constrained light response.

---

## 18. Sprite-level VFX

Aestra should affect existing game sprites, not only spawn particles around them.

Potential sprite modifiers:

```text
Flash
Tint
PaletteSwap
Outline
Glow
Dissolve
Burn
Wave
Skew
Squash
Stretch
Shake
PixelScatter
MaskReveal
```

Example:

```text
PlayerHit
|
+-- SpriteModifier
|     +-- WhiteFlash
|     +-- HorizontalSquash
|     +-- OutlinePulse
|
+-- SparkEmitter
|
+-- CameraShake Output
```

---

## 19. Pixel trails, ribbons, and beams

Presentation modes:

```text
Smooth
Segmented
PixelStepped
SpriteStamped
```

Concept:

```text
continuous spline
    ->
sample every N pixels
    ->
quantize coordinates
    ->
hard-edged rasterization
```

Useful for lightning, sword trails, beams, ropes, and projectiles.

---

## 20. 3D-to-pixel VFX

Mesh rendering should remain legal in pixel projects.

Example:

```text
MeshRenderer
    Mesh       Sphere
    Material   ArcaneEnergy

PixelArtProfile
    Resolution      64x64
    Palette         5 colors
    Filter          Nearest
```

This enables procedural 3D VFX with pixel-art presentation.

---

## 21. Effect-local pixel render targets

Allow an effect to render into a low-resolution surface:

```text
Aestra Effect
    ->
Low Resolution Target
    ->
Palette / Dither
    ->
Nearest Upscale
```

Candidate sizes:

```text
16x16
32x32
64x64
128x128
custom
```

Use for runtime presentation, baking, and hybrid 3D-to-pixel effects.

---

## 22. Runtime and baking must share one authored effect

One `EffectAsset` should support both:

```text
Runtime -> Bevy / Godot / Unity / ...
Bake    -> Sprite Sheet / PNG Sequence / FlipbookAsset
```

Bake options:

```text
resolution
frame rate
duration
loop duration
palette
dithering
alpha
packing
padding
pivot
metadata
```

---

## 23. Simulation-to-flipbook baking

Major target feature:

```text
High-quality Aestra Effect
|
+-- particles
+-- ribbons
+-- meshes
+-- Grid2D smoke
+-- lighting
+-- distortion
|
v
Offscreen Simulation
|
v
Pixel Presentation
|
v
Capture Frames
|
v
Atlas Packing
|
v
FlipbookAsset
```

This enables expensive procedural effects to be deployed cheaply.

---

## 24. Loop authoring

Add explicit loop tools:

```text
Loop Duration
Loop Preview
Loop Error Visualization
Loop Seed
Seamless Bake
```

Potential helpers:

```text
periodic noise
time wrapping
loop-safe emitter scheduling
loop-safe random streams
```

---

## 25. 2D data interfaces

Recommended semantic interfaces:

```text
Transform2D
Camera2D
SceneColor2D
SceneDepth2D
SceneNormals2D
SpriteTexture
SpriteMask
Physics2D
Grid2D
SceneSDF2D
Tilemap2D
```

Engine adapters provide concrete implementations.

---

## 26. SceneSDF2D

Potential API:

```text
SceneSDF2D.sample(position)   -> distance
SceneSDF2D.gradient(position) -> normal
```

Use cases:

```text
soft shadows
contact glow
outlines
collision distance
shockwaves
distance dissolve
shore foam
light occlusion
particle avoidance
surface attachment
```

GPU distance-field techniques such as Jump Flooding are useful references.

---

## 27. Grid2D simulation domain

Reserve:

```text
SimulationDomain
    Particle
    Strip
    Grid2D
    Grid3D
    Custom
```

Potential Grid2D channels:

```text
velocity
pressure
density
temperature
fuel
color
custom0..N
```

Use cases:

```text
smoke
fire
fog
water
poison
blood
reaction-diffusion
heat haze
magic fields
```

---

## 28. Particle <-> Grid interaction

Support bidirectional coupling:

```text
Particle -> Grid
    inject density
    inject heat
    inject force

Grid -> Particle
    sample velocity
    sample density
    spawn particles
    alter color
    alter lifetime
```

Avoid treating “fluids” as a completely isolated subsystem.

---

## 29. Pixel fluids

Recommended principle:

> Simulate continuously; stylize during presentation.

```text
continuous fluid/advection
    ->
density field
    ->
threshold / quantize / palette
    ->
pixel-art smoke/fire/water
```

Animal Well is an important reference for this design philosophy.

---

## 30. 2D collision and tilemaps

Potential collision sources:

```text
Physics2D
SpriteMask
SceneSDF2D
Tilemap
CustomCollider
```

Collision payload:

```text
position
normal
velocity
surface/material
entity
```

Use normal typed Aestra events.

---

## 31. Preview modes

Editor preview modes:

```text
3D Preview
2D Preview
Pixel Preview
Hybrid Preview
```

Pixel preview controls:

```text
Resolution
Pixels/Unit
Zoom
Pixel Grid
Palette
Dither
Presentation FPS
```

Useful display modes:

```text
Native
Pixel
Split
```

Split view should compare continuous simulation to final pixel presentation.

---

## 32. 2D gizmos and camera

Required 2D gizmos:

```text
Position XY
Rotation
Scale
Emitter Shape
Beam Endpoints
Ribbon Control Points
Collider Bounds
Light Radius
Grid2D Domain
```

2D camera:

```text
pan
zoom
frame selection
pixel-perfect zoom steps
```

Hybrid profile may switch between orthographic 2D, orthographic 3D, and perspective without changing the effect.

---

## 33. Asset browser additions

The Project Assets toolbar can show all dimensions or filter to saved 2D or 3D effects.
Set **Dimension** in the effect's Properties and save to update its classification. Untagged
effects default to 3D and the bundled pixel and sprite samples are tagged 2D.

Categories:

```text
Sprites
Flipbooks
Palettes
Materials
Textures
Meshes
Pixel Presets
```

Useful sprite metadata:

```text
resolution
pivot
alpha mode
recommended filtering
normal map
palette
```

---

## 34. Aseprite integration

Future optional integration:

```text
import sprite
animation tags
frame durations
layers
slices/pivots
palette
```

Potential bake output back to Aseprite-compatible spritesheet workflows.

---

## 35. Diagnostics

Pixel-specific diagnostics should include:

```text
WARNING: Sprite uses linear filtering in PixelArt profile.
WARNING: Particle size is below one virtual pixel.
WARNING: Flipbook interpolation is enabled.
WARNING: Screen-space dither may swim during camera movement.
WARNING: Smooth gradient detected with no palette quantization.
WARNING: Presentation FPS does not divide loop duration cleanly.
WARNING: Sprite normal-map size differs from albedo size.
WARNING: Pixel render target is unusually large.
```

---

## 36. Profiling

Track:

```text
particle count
sprite count
flipbook texture memory
overdraw
transparent pixel ratio
render-target resolution
palette pass cost
dither cost
Grid2D cell count
Grid2D solver cost
draw calls
batch count
```

Pixel art is not automatically cheap.

---

## 37. Quality profiles

Allow per-platform substitutions.

Example:

```text
Desktop
    runtime Grid2D smoke

Steam Deck
    lower Grid2D resolution

Mobile
    baked flipbook
```

Quality profiles may override:

```text
renderer
asset
grid resolution
particle count
material features
presentation resolution
```

---

## 38. Plugin architecture

Plugins may contribute:

```text
Renderer
SimulationDomain
DataInterface
MaterialNode
Module
AssetType
Baker
Importer
Diagnostic
ProfilerPanel
```

Examples:

```text
FLIP2D plugin
CellularAutomata plugin
PixelOutline renderer
Custom dither plugin
```

---

# 39. Milestones

### First editor-to-host slice

Open **Pixel Pulse** in the bundled sample project and use the viewport's **3D** button to switch to the orthographic XY view. Middle-drag pans, the wheel zooms, and Frame Effect centers the view. The preview choice belongs to the editor session.

`cargo run --locked -p aestra-bevy --example pixel_pulse` opens the same saved effect in a Bevy `Camera2d` scene. Two effect instances share the existing alpha flipbook material with nearest filtering. **Up** raises the left pulse above the foreground sprite and **Down** lowers it behind the background sprite. Press **Y** to sort by world Y, then use **Up** and **Down** to move the pulse across the scene. **R** resets its position and sort policy.

This slice covers native GPU sprite and flipbook playback. Pixel snapping, integer scaling, and the effect-local compositing surface have separate acceptance criteria below.

## M0 — Architecture contract — P0

Tasks:

- define `AuthoringProfile`;
- confirm one `EffectAsset`;
- define `SpatialConstraint`;
- define presentation profiles;
- document Bevy independence;
- document simulation precision vs presentation precision.

Acceptance:

- [ ] No `Effect2D` / `Effect3D`.
- [ ] No Bevy `Core2d/Core3d` identifier in core assets.
- [ ] Pixel presentation is separate from simulation.
- [ ] Project profile is editor/default configuration only.

---

## M1 — 2D preview foundation — P0

The orthographic XY view uses the existing Grid toggle. Its red X and green Y axes stay fixed at world zero, and grid subdivisions fade between zoom levels. The grid draws behind effects and follows the docked camera viewport when it resizes.

Click an emitter's origin marker to select it. The Frame button fits the selected emitter's spawn shape and sprite size. Move works on X, Y, or the XY plane. Rotate uses world Z, and Scale edits the two planar axes. Drags preview live and commit once on release. Escape cancels the preview, and Properties remains available for layer Z.

Tasks:

- [x] orthographic 2D camera;
- [x] adaptive XY grid with world axes;
- [x] emitter selection from the 2D viewport;
- [x] planar transform gizmos;
- [x] frame selected emitter;
- authoring-profile switching.

Acceptance:

- [ ] 2D authoring does not require perspective-camera interaction.
- [ ] Z remains editable.
- [ ] Profile changes do not rewrite effect semantics.

---

## M2 — SpriteRenderer — P0

Use the existing sprite and flipbook renderer plans, materials, and per-emitter renderer lists as the baseline.

Tasks:

- fixed-plane orientation;
- additional size modes;
- pivot;
- qualify existing blending, filtering, batching, and culling on a 2D camera.

Acceptance:

- [ ] Particle simulations render as sprites.
- [ ] The same simulation can switch SpriteRenderer <-> MeshRenderer.
- [ ] Multiple renderers can consume one simulation.

---

## M3 — 2D sorting and layers — P0

Native 2D effects default to their propagated world Z. A host can insert `AestraSort2d` on the
effect entity and change or remove it during playback. `WorldY { units_per_y }` uses
`world_z - world_y * units_per_y`, so a positive scale draws lower effects in front.
`Custom(key)` accepts a host-computed value. The Bevy adapter submits the resolved key in the
same phase as ordinary sprites, where larger keys draw later. Use distinct keys when the host
needs explicit ordering between entities. Authored renderer order breaks ties within one effect.

Use the host's `RenderLayers` component on the effect entity to select cameras. Its sort key
controls compositing within each selected camera. Sort policy and layer selection belong to
the host instance. The effect asset and core model remain engine-neutral.

In the rendered XY viewport, **Sort Order** lists active effect instances front to back,
with their resolved key, policy, and camera layers. The readout uses the same placement as
the renderer, including authored and inherited host motion. Enable **World Y** to preview
the Y policy at one sort unit per world unit. Closing the overlay or leaving the rendered
XY view restores the previous policies. This works with the regular and pixel preview
cameras and does not edit or save the effect.

Acceptance:

- [x] Pixel Pulse overlaps native Bevy sprites using world Z, world Y, and custom keys.
  The native capture checks live policy changes and removal while holding world Z fixed.
- [x] Ordering semantics remain engine-neutral.
- [x] The editor shows 2D sort placement and camera layers and can preview World Y ordering.

---

## M4 — Flipbook assets and renderer — P0

Keep `FlipbookDefinition` and its explicit normalized frame rectangles. A regular grid or packed atlas importer can populate the same representation. Frame hold, particle-age and effect-time clocks, playback direction, and random start already exist.

In **Assets → Current Document**, select a registered texture and choose **Create Flipbook**.
Set columns, rows, the number of populated frames, and FPS. The preview strip shows
frame order from the top-left of the texture. Create adds one undoable flipbook definition.
Native GPU playback accepts up to 64 frames per referenced flipbook. Unused definitions
do not affect backend selection. Legacy native flipbooks sample with nearest filtering.
Semantic materials continue to use their authored sampler. Native sprite UVs now use
the same top-left origin as the Bevy sprite path.

Tasks:

- qualify the existing frame-hold playback and nearest texture filtering in the editor and host;
- extend the atlas importer and frame metadata for packed sprites;
- evaluate explicit start frames and optional frame interpolation after the held-frame workflow works.

Acceptance:

- [x] Regular-grid flipbooks retain frame rectangles through save and reopen and animate in native playback.
- [ ] Pixel projects default to non-interpolated frame sampling.

---

## M5 — Pixel Art Presentation v1 — P0

Start with a host-owned virtual viewport. Define the snap origin and rounding in virtual pixel coordinates after the camera transform, and use the same settings in the editor preview. Verify camera pan, window resize, and integer upscale. M13 owns effect-local targets and their alpha and scene ordering contract.

The opt-in `PixelGrid` camera component snaps native GPU sprite and flipbook centers to the nearest virtual pixel edge, measured from the top left of the 2D viewport, with ties rounding right and down. Pixel Pulse and the editor's **PX** preview render at 192×128, start at one world unit per virtual pixel, and composite with nearest filtering and integer letterboxing. The editor maps picking to the fitted image and draws gizmos at window resolution. A dock smaller than 1× uses the regular 2D view until the pixel preview fits again.

Tasks:

- `PixelArtProfile`;
- virtual resolution;
- pixels/unit;
- nearest defaults;
- render-position snapping;
- no-AA mode;
- no-mipmap option;
- pixel preview.

Acceptance:

- [x] Continuous simulation can present as snapped pixel output.
- [x] Native GPU sprite and flipbook renderers can use the host's virtual pixel grid.
- [x] The editor's fixed-resolution preview matches the Pixel Pulse host presentation at the same camera pose and zoom.

---

## M6 — Temporal presentation — P1

`PresentationCadence` applies to native GPU sprites and flipbooks in analytic and independent stateful effects. **12 FPS** in the editor's **PX** preview and **T** in the Pixel Pulse host use the same effect-relative sample clock. The host's `--stateful` option exercises the persistent solver. Stateful emitters publish the latest due pose into their existing draw buffers and continue integrating through the live tick. Enabling during playback shows the current pose until the next boundary. A backward seek reconstructs both the held frame and live state. The sampler is independent of export cadence. Coupled and mixed simulations, trails, and the baker remain follow-up work.

Tasks:

- presentation FPS;
- deterministic stepping;
- preview controls;
- bake-rate separation.

Acceptance:

- [x] A 60 Hz playhead can present analytic and independent stateful sprites and flipbooks at 12 FPS.
- [ ] Runtime and baker share timing semantics.

---

## M7 — Palette system — P1

The first slice imports GIMP `.gpl` palettes from the Project Assets context menu. The importer
stores up to 256 ordered sRGB swatches as a one-row `.palette.png`, so the same asset can be
assigned to multiple material instances. Add **Nearest Palette Color** to the material graph,
connect its color output, and drop the imported texture onto the renderer's **Palette** property.
The node matches linear RGB and carries through the input alpha. Pixel Pulse uses the same
project palette in its editor preview and Bevy example.

**Palette Ramp** maps a scalar **Factor** to equal bands across the same ordered swatches.
It clamps to the first and last swatch at the endpoints and uses a grayscale ramp while the
palette is unbound. Drive it from a scalar such as **ParticleNormalizedAge** and use the material's
alpha output to author transparency independently.

**Palette Shift** finds the nearest swatch to its **Color** input, then advances by **Steps**
in either direction, wrapping around the ordered swatches. The node truncates fractional steps
toward zero and preserves the input color's alpha. Drive **Steps** with **EffectTime** multiplied by
a signed rate in swatches per second to cycle. For example, a rate of `-4` moves backward one
swatch every quarter second. An unbound palette passes the incoming color through.

Open a `.palette.png` in **Project Assets** or choose **Edit Palette** to open the dockable
**Palette Editor**. Select, add, remove, or reorder swatches and edit the selected swatch with
the Feathers color picker using RGB, HSL, or hex. Effects using the palette preview the draft.
**Discard Changes** restores the saved colors, and **Save Palette** updates the file at the same
path so existing material bindings continue to work. Closing the tab pauses preview and keeps
the draft available in the current project.

Tasks:

- `PaletteAsset`;
- GIMP import and in-editor swatch authoring;
- lookup;
- stepped ramps through the **Palette Ramp** material node;
- signed shifts and time-driven cycles through **Palette Shift**;
- ordered swatch preview and live effect preview.

Acceptance:

- [x] Effects can be constrained to a selected palette. Pixel Pulse's native editor thumbnail
  and Bevy host captures verify the rendered swatches.
- [x] Palette assets are reusable through project texture bindings.
- [x] Draft swatch edits preview on Pixel Pulse before saving, and discarding restores the
  original render in the native GPU capture.

---

## M8 — Dithering — P1

**Bayer Dither 2×2**, **4×4**, and **8×8** turn fractional **Alpha** into binary coverage. Connect
the node to the material's alpha output, or multiply it by the sprite's existing alpha to keep
the silhouette. Larger tiles provide finer coverage steps, from 4 cells in 2×2 to 64 in 8×8.
**Cell Size** sets how many coordinate units a cell covers. **Position** defaults to
`EffectPositionXY`, which follows the owning effect's transform. Connect `WorldPositionXY` for a
world-anchored pattern or `ScreenPixel` for a render-target-aligned pattern. In Pixel Grid mode,
screen cells use virtual pixels before integer upscaling. For sprites and flipbooks,
`ParticlePositionXY` starts at each particle's center and follows its quad axes in authored
particle-size units. It stays aligned to the quad as it moves or rotates and uses the same
coordinates across atlas frames.

Set Pixel Pulse's **Dither Coverage** property to `0.5` in the editor. The Bevy example uses the
same authored material with `cargo run --locked -p aestra-bevy --example pixel_pulse -- --dither`.

**Blue Noise Dither** accepts the same three inputs. It uses a fixed, seamless 32×32 rank
tile with 1,024 coverage steps. The editor's graph previews and the generated shaders read
the same rank data without a material texture binding. To try the pattern in Pixel Pulse,
replace the Bayer node with **Blue Noise Dither** and reuse its connections, or launch the
Bevy example with `--blue-noise`.

**Pattern Dither** adds a **Texture** input alongside **Alpha**, **Position**, and **Cell Size**.
Assign a PNG with varying alpha to **Dither Pattern** on the material instance. The texture's
alpha channel stores the thresholds. A cell stays visible when incoming alpha exceeds its
threshold. Incoming alpha of zero always hides the cell and one always shows it. Rectangular
tiles repeat in both directions, with one texel per cell. An empty or loading pattern passes
through the incoming alpha. Connect the result to the material's alpha, or multiply it by the
sprite's alpha to preserve the silhouette. The native editor capture checks a 3×2 tile at two
pixels per cell before and after moving Pixel Pulse, and checks half opacity with an empty slot.

Native editor and Bevy captures check all four anchors against the CPU mask at 30% coverage.
Between captures the camera pans, the effect moves, the particle moves within the effect,
and the flipbook switches to its second atlas frame.

Tasks:

- [x] Bayer 2×2, 4×4, and 8×8
- [x] Blue noise
- [x] Custom pattern
- [x] Screen, world, effect-local, and particle-local anchoring for native sprites and flipbooks.

Acceptance:

- [x] World and effect patterns stay anchored to their respective coordinate systems.
- [x] The live editor viewport and Bevy match the same four coordinate references for Pixel Pulse.

---

## M9 — Pixel material nodes — P1

**Pixel Snap** returns the center of the XY cell containing **Position**. The **Cell Size**
input uses the units of the connected coordinate, such as effect units for `EffectPositionXY`
or virtual pixels for `ScreenPixel`. Feed the snapped coordinate into a texture or noise node
so every fragment in a cell samples the same location. **Posterize** rounds linear RGB to the
nearest of the specified **Levels** between black and white, extends those steps into HDR,
and preserves alpha.

**Pixelate UV** samples at cell centers on a normalized UV grid. **Cells** sets columns
and rows independently across the full texture. For an atlas, align cell boundaries
with frame boundaries. **Quantize** rounds a scalar to the nearest multiple of a
positive **Step**. A zero step leaves the input as-is. **Threshold** produces a hard
mask that is one at or above the cutoff and zero below it.

Open **Pixel Dissolve** in the sample project and enable **PX** in the XY viewport. Its
material snaps particle-local coordinates before evaluating **Value Noise 2D**. The noise
drives the hard **Threshold** mask and the color passed to **Posterize**. The instance
exposes **Cell Size**, **Posterize Levels**, and **Atlas Cells**. **Progress Step**
defaults to 1/16. Set it to zero for continuous progress. Turn off **Auto Dissolve** to
scrub **Dissolve Progress** manually from zero (intact) to one (gone).

The native XY viewport test captures all three dissolve phases and checks whole-cell coverage
and the configured five color levels. The saved material also compiles through the portable
shader path.

**Pixel Outline** returns an exterior mask using the source texture's alpha. **Width**
rounds to 0–4 texels, with zero disabling the outline. **Threshold** sets the minimum
alpha for the silhouette. A one-texel outline uses the four cardinal neighbors.
Flipbook sampling stays inside the active frame. Leave transparent padding in each
frame for the outline to appear at the edges of the silhouette.

Open **Pixel Outline** in the sample project and enable **PX** in the XY viewport.
The sample applies the mask to a two-frame flipbook. **Outline Width**, **Alpha Cutoff**,
and **Outline Color** are editable on the material instance. The native XY viewport test
checks both frames against an atlas with a solid texel immediately across the frame boundary.

Initial nodes:

```text
PixelCoordinate
PixelSnap
Posterize
Quantize
Threshold
PaletteLookup
BayerDither
PixelNoise
PixelOutline
PixelDissolve
PixelateUV
```

Acceptance:

- [ ] Nodes compile through normal material IR.
- [ ] Runtime and baking both support them.

---

## M10 — Sprite modifier / sprite input — P1

The Bevy integration exposes `SpriteTextureInput::resolve`. It snapshots the image
handle, normalized UV origin and signed extent, source rectangle size in texels,
local quad size, and anchor offset for ordinary image sprites. This includes
atlas frame selection, sprite crops, custom size, and both flips.

The `sprite_hit_flash` example triggers a short-lived overlay on an existing
axis-aligned Bevy sprite. The host owns the trigger, source sprite, and overlay
lifetime. The editor's **Sprite Hit Flash** sample exposes the flash color in the
material and the fade in the emitter's Appearance module. The material uses a
preview atlas frame in the editor and samples the host's selected texture and UV
rectangle in the example. Open the effect in the sample project and restart
playback to see the one-shot.

Press **O** in the same example for **Sprite Outline**. The Pixel Outline node's
optional **Frame Bounds** input controls where it can read source texels. With
explicit bounds and a padded quad, it can draw past the frame edges without
reading adjacent atlas tiles. A disconnected input uses the current renderer
frame and confines the outline to that frame. The editor sample exposes outline
color, width (one to four source texels), and alpha threshold.

Press **D** for **Sprite Dissolve** and **R** to restore the original sprite.
The effect keeps the source frame's color, tint, and alpha and removes whole
texture cells. The editor sample exposes cell size in source pixels, dissolve
start and finish, and progress step. The host can request native pipeline
preparation on a `Camera2d` and keep its original sprite visible while the
request is pending. During playback, the effect takes over the sprite's draw
and reveals the scene behind it as cells disappear.

Press **P** for **Sprite Palette Swap**. It keeps the recolored frame visible
until the host restores the sprite with **R**. The **Palette Swap** node maps
matching RGB swatches by index between two same-width, one-row sRGB palettes.
It preserves source alpha and passes through unmatched colors and unusable
palette pairs. The sample applies the host tint after the lookup. Both
palettes are editable through the Asset Browser and assignable on the material.

Press **W** for **Sprite Distortion**. A horizontal wave warps the selected
atlas crop in sprite-local UVs. The reusable **Sprite Wave UV** material
function returns inset sampling coordinates and frame coverage. The sample
uses nearest sampling and clips coverage outside the selected frame. Its
material exposes strength in source pixels, wave count, and speed. The wave
settles before the host restores the original sprite automatically. **R**
restores it immediately.

- [x] Use an existing host sprite's texture and alpha for a hit flash.
- [x] Keep the trigger and the source sprite under host control.
- [x] Draw an exterior outline around host sprites without atlas bleed.
- [x] Dissolve a host sprite with editable pixel cells and explicit host restoration.
- [x] Swap a host sprite's palette by swatch index while retaining unmatched colors and alpha.
- [x] Distort a host sprite's own texture with atlas-safe sampling and automatic restoration.

The example captures the frame on trigger and uses axis-aligned image sprites.

---

## M11 — 2D lighting — P1

The native GPU path now lets sprite and flipbook materials receive linear RGB
irradiance through the existing Scene Point Irradiance input. Bevy hosts attach
`AestraLights2d` to a `Camera2d` and update each light's world-space XY
position, radius, color, intensity, and falloff. Lights can use continuous
radial attenuation or any positive number of discrete bands. A camera with no
lights supplies zero irradiance, so materials can author their own ambient
color.

Open `sample-project/effects/pixel_lighting.aestra.ron` in the editor and use
**LGT** in XY rendered mode to cycle the moving preview light through
continuous falloff, four bands, and off. The regular and virtual-pixel preview
cameras use the same light. Run the same authored effect in Bevy with
`cargo run --locked -p aestra-bevy --example pixel_lighting` and press Space
to cycle modes.

**Sprite normals**

Connect a linear-data normal atlas to **Normal Map**, then connect its output
to the **Normal** input of **Sprite Point Irradiance**. Share the sprite's
atlas UVs between its color and normal sampling. The light uses the rendered
quad's orientation and evaluates its normal response before applying any
falloff bands. It uses a virtual light elevation of half the light radius.
Add the resulting linear RGB to the material's ambient contribution.

Open `sample-project/effects/pixel_normal_lighting.aestra.ron` and enable
**LGT** in the XY rendered viewport. To use the same effect in the Bevy
example, append `-- --normals` to the command above. Both sprite and flipbook
normal maps work in the standard and virtual-pixel `Camera2d` views.

**Palette-constrained lighting**

The Pixel Palette Lighting sample adds ambient RGB to point irradiance and
passes the sum through the reusable **Linear Luminance** material function.
The **Light Gain** parameter scales that level before **Palette Ramp** selects
an sRGB swatch from the ordered **Light Ramp** texture. The first swatch is the
unlit look at the sample's default ambient level. The atlas supplies opacity
independently of the ramp. Opaque texels use the swatches directly, while
translucent texels blend with the background.

Open `sample-project/effects/pixel_palette_lighting.aestra.ron` and enable
**LGT** in the rendered XY viewport. Use `-- --palette` in the Bevy example
to run the same material. Both continuous and stepped light falloff can drive
the ramp.

**Remaining work**

- [ ] Shadow-mask interface.

**Acceptance**

- [x] Sprite and flipbook particles can receive a moving 2D point light.
- [x] Pixel lighting can use discrete falloff bands.
- [x] Sprite and flipbook normal maps respond to the light's direction.
- [x] Point lighting moves opaque sprite and flipbook texels between authored palette swatches.

---

## M12 — Pixel trails and beams — P1

Tasks:

- segmented ribbons;
- pixel-stepped paths;
- sprite-stamped paths;
- quantized beam coordinates.

Acceptance:

- [ ] Pixel lightning can be authored without baked sprites.
- [ ] Smooth and pixel-stepped presentation are interchangeable.

---

## M13 — Effect render targets — P1

The bake path uses the same `pixel_render_target` image allocation as the editor's
pixel viewport. Bevy hosts can add the image to `Assets<Image>` and use the handle
as the target of a `Camera2d` or `Camera3d`. The image uses nearest filtering and
supports readback. Transparent camera clears produce premultiplied RGB under
source-over blending, so runtime consumers should composite accordingly.

Tasks:

- offscreen target;
- configurable resolution;
- alpha;
- post-presentation;
- nearest upscale.

Acceptance:

- [x] An unlit sprite effect can render through an orthographic 3D camera to a configurable pixel surface.
- [x] The target feeds the baker. The native test captures a tilted view and reimports the atlas for 2D playback at the selected world span.

---

## M14 — Simulation-to-flipbook baker — P1

Begin with self-contained unlit effects. A reproducible bake fixes the timestep, seed, camera, backend, and inputs. Specify alpha compositing and additive playback before extending to effects that consume captured host or scene inputs.

The headless bake captures an analytic alpha sprite effect at 12 FPS. It defaults to a
transparent 64×64 XY target centered at the effect origin with one world unit per pixel.
The output is a new directory containing `textures/atlas.png`
and `effects/flipbook.aestra.ron`. Open the output directory in Aestra and load
the generated effect to preview the baked playback.

```powershell
cargo run --locked -p aestra-viewer -- --bake-flipbook sample-project/effects/pixel_outline.aestra.ron target/pixel-outline-bake
```

Use `--size` to set the side length in pixels, `--span` to set the view width and height
in world units, and `--center X,Y,Z` to move the frame. Add `--eye X,Y,Z` to render
from an orthographic 3D camera looking at that center. The exported sprite places
the image center at its local origin and spans the selected world width.

The bake captures the range from zero up to the effect duration and preserves the
source's restart or one-shot playback mode. The exported atlas uses straight alpha.
The bake supports up to 64 frames and checks the packed atlas against the render
device's texture limit. Cropping, padding, additive playback, and stateful effects
remain follow-up work.

Tasks:

- deterministic offline simulation;
- capture;
- crop;
- packing;
- padding;
- pivot metadata;
- palette/dither processing;
- `FlipbookAsset` generation.

Acceptance:

- [x] A self-contained analytic alpha effect can be baked and imported as a flipbook.
- [x] Repeated native captures under the same bake configuration match on the same adapter.
- [x] The asymmetric multi-row round-trip test checks frame order, orientation, timing, and alpha playback.

---

## M15 — Loop authoring — P1

Tasks:

- loop range;
- loop preview;
- seam diagnostics;
- periodic-noise helpers;
- seamless bake.

Acceptance:

- [ ] Loop seams can be inspected and validated.

---

## M16 — SceneSDF2D — P2

Tasks:

- engine-neutral interface;
- Bevy implementation;
- distance sampling;
- gradient/normal sampling;
- debug visualization.

Acceptance:

- [ ] Materials and modules can sample 2D scene distance.
- [ ] Core is independent of Bevy implementation details.

---

## M17 — 2D collision and tilemaps — P2

Tasks:

- Physics2D;
- tilemap interface;
- sprite-mask collision;
- SDF collision;
- typed collision events.

Acceptance:

- [ ] Particles can collide with common 2D scenes.
- [ ] Tile/surface metadata can affect VFX.

---

## M18 — Grid2D infrastructure — P2

Tasks:

- Grid2D simulation domain;
- GPU storage;
- channels;
- scheduling;
- module API;
- plugin hooks;
- debug view.

Acceptance:

- [ ] Grid2D can execute as a simulation stage.
- [ ] Plugins can define solvers.
- [ ] Particle modules can sample Grid2D.

---

## M19 — Grid2D smoke / advection — P2

Tasks:

- velocity;
- density;
- advection;
- dissipation;
- particle/grid injection;
- pixel presentation.

Acceptance:

- [ ] Interactive smoke runs in real time.
- [ ] Grid resolution is quality-scalable.

---

## M20 — Advanced 2D fluids — P3

Candidates:

```text
FLIP
stable fluids
fire combustion
reaction-diffusion
cellular materials
```

Do not implement everything at once. Keep solver APIs plugin-friendly.

---

## M21 — Aseprite integration — P3

Tasks:

- sprite import;
- tags;
- durations;
- layers;
- slices/pivots;
- palettes;
- baked-animation export.

---

## M22 — AI-aware pixel authoring — P3

Structured operations:

```text
set_pixel_profile
set_palette
set_dither
set_presentation_fps
assign_sprite
assign_flipbook
create_pixel_material
bake_flipbook
optimize_for_pixel
```

AI diagnostics:

```text
pixel density
palette mismatch
filter mismatch
frame interpolation
temporal instability risk
overdraw
grid cost
```

---

# 40. Dependency order

```text
M0 Architecture
      |
M1 2D Preview
      |
M2 SpriteRenderer
      |
 +----+----+
 |         |
M3        M4
Sorting   Flipbook
 |         |
 +----+----+
      |
M5 Pixel Presentation
      |
 +----+----+
 |         |
M6        M7
Timing    Palette
           |
          M8 Dither
           |
          M9 Materials
      |
 +----+----------------+
 |                     |
M10 Sprite Mods     M11 Lighting
 |                     |
 +----------+----------+
            |
          M12 Trails
            |
          M13 Render Targets
            |
          M14 Baker
            |
          M15 Looping
            |
      +-----+------+
      |            |
    M16 SDF      M18 Grid2D
      |            |
    M17          M19 Smoke
                   |
                 M20 Fluids
```

---

# 41. Priority buckets

## P0 — Credible 2D / pixel support

```text
M0 Architecture
M1 2D Preview
M2 SpriteRenderer
M3 Sorting
M4 Flipbook
M5 Pixel Presentation
```

## P1 — Differentiated pixel-VFX tool

```text
M6 Temporal Presentation
M7 Palette
M8 Dithering
M9 Pixel Materials
M10 Sprite Modifiers
M11 Lighting
M12 Pixel Trails
M13 Render Targets
M14 Flipbook Baker
M15 Loop Authoring
```

## P2 — Advanced interactive simulation

```text
M16 SceneSDF2D
M17 Collision / Tilemaps
M18 Grid2D
M19 Smoke / Advection
```

## P3 — Advanced ecosystem

```text
M20 Advanced Fluids
M21 Aseprite
M22 AI Pixel Authoring
```

---

# 42. Recommended immediate implementation sequence

Do **not** begin with fluids.

Implement:

```text
1. 2D Preview
2. SpriteRenderer
3. 2D Sorting
4. FlipbookRenderer
5. PixelArtProfile
6. Pixel Preview
7. Presentation FPS
8. PaletteAsset
9. Dithering
10. Pixel Material Nodes
```

Only then proceed to:

```text
SceneSDF2D
Grid2D
fluids
advanced collision
```

This provides useful 2D/pixel support early while preserving the long-term architecture.

---

# 43. Reference links

### Tools

- Pixel Composer: https://docs.pixel-composer.com/
- SpriteMancer: https://spritemancer.com/
- SpriteMancer particles: https://spritemancer.com/docs/particles.html
- SpriteMancer fluids: https://spritemancer.com/docs/fluids.html
- Pixel FX Designer: https://store.steampowered.com/app/939360/Pixel_FX_Designer/
- Pixelpart: https://pixelpart.net/documentation/book/
- Pixelpart inputs: https://pixelpart.net/documentation/book/advanced/effect-inputs.html
- Pixelpart rendering/baking: https://pixelpart.net/documentation/book/rendering-effects.html
- Effekseer: https://effekseer.github.io/en/
- Juice FX: https://store.steampowered.com/app/1046770/Juice_FX/
- Unity 2D VFX Graph compatibility: https://docs.unity3d.com/Packages/com.unity.render-pipelines.universal@15.0/manual/2d-visual-effect-graph-compatibility.html
- Niagara flipbook baker: https://dev.epicgames.com/documentation/en-us/unreal-engine/niagara-flipbook-baker-quick-start-guide-in-unreal-engine
- Godot Particles2D: https://docs.godotengine.org/en/latest/tutorials/2d/particle_systems_2d.html

### Games / rendering references

- Animal Well rendering: https://blog.playstation.com/2022/07/20/how-animal-well-taps-into-ps5-hardware-to-elevate-2d-pixel-art-platforming/
- Animal Well engine discussion: https://www.gamedeveloper.com/design/why-animal-well-s-home-brewed-engine-was-key-to-its-success
- Eastward art/rendering: https://www.gamedeveloper.com/art/eastward-s-creators-share-insights-on-making-pixel-art-adventures
- Dead Cells 3D-to-2D pipeline: https://www.gamedeveloper.com/production/art-design-deep-dive-using-a-3d-pipeline-for-2d-animation-in-i-dead-cells-i-
- Sea of Stars: https://sabotagestudio.com/presskits/sea-of-stars/
- Noita: https://www.noitagame.com/
- Never's End GDC 2026 talk: https://schedule.gdconf.com/session/how-we-draw-a-3d-sprite-world-the-stylized-art-of-nevers-end/914052

### Algorithms / research

- Jump Flooding Algorithm: https://doi.org/10.1145/1111411.1111431
- Real-time SDF / soft-shadow reference: https://arxiv.org/abs/2210.06160
- GDC Visualizing Math: https://www.gdcvault.com/play/1027759

---

# 44. Final architectural position

Aestra should not become:

```text
Aestra 2D
+
Aestra 3D
```

It should become:

```text
                     Aestra
                       |
                Semantic Effect
                       |
          +------------+------------+
          |                         |
      Simulation                 Renderer
          |                         |
   +------+------+          +-------+---------+
   |      |      |          |       |         |
Particle Strip Grid2D     Sprite   Mesh     Ribbon
          |                         |
          +------------+------------+
                       |
                 Presentation
                   /       \
             Standard    Pixel Art
                           |
                 +---------+----------+
                 |         |          |
              Palette   Dither   Pixel Timing
                           |
                           v
                    Runtime / Bake
```

This keeps Aestra:

- engine-neutral;
- compatible with Bevy's evolving renderer;
- suitable for 2D, 3D, and 2.5D;
- capable of authentic pixel-art output;
- future-compatible with Grid2D/fluid simulation;
- friendly to baking and optimization;
- structured enough for AI authoring.

> **The simulation can be modern, continuous, and high fidelity. The presentation decides whether the final result looks smooth, painterly, sprite-based, or deliberately pixelated.**
