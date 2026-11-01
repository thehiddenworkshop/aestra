# Water as an effect-lighting receiver

The editor and offshore fireworks demo share
`aestra_bevy::preview::water::WaterMaterial`. This is a host-owned horizontal Y-up opaque surface,
separate from the effect's particle/material graph and simulation state.

In the editor, enable **Settings → Preview → Water surface**. It replaces the
grid until disabled, preserving the grid preference. Enable **Photographic
preview (HDR)** for the same display response used by the demo. Saved scene
outputs can illuminate the water through the existing scene-lighting path.
Water is off by default, and old settings files keep that default.

In the demo, use `--scene sea --water calm` or `--scene sea --water rough`.
`--no-launch-lights` disables only the gameplay rocket's moving light, providing
a control for its water highlight. The frozen calibration/showcase remains
unchanged. The review runner accepts `-Scene sea -Water calm|rough`.

The material uses eight directional wave bands with wavelengths from 18 to
0.09 world units. Assuming metre-scaled worlds, wave speeds follow deep-water
gravity-wave dispersion. Resolved bands perturb the lighting normal. Subpixel
bands are filtered out and their slope variance broadens microfacet roughness,
instead of creating unresolved glitter. Water/air IOR 1.333 sets normal-incidence
reflectance to approximately 0.02037. Bevy supplies normal clustered PBR lighting,
including Aestra's existing selected-star and representative-light adapters.

```rust,ignore
use aestra_bevy::preview::water::{self, SeaState, WaterMaterial, WaterMaterialPlugin};

// Register once after Bevy's normal rendering plugins.
app.add_plugins(WaterMaterialPlugin);

// Assign this material to the host's water mesh.
let mut surface: WaterMaterial = water::material(SeaState::Rough);
surface.extension.set_time(effect_elapsed_seconds);
```

Supply the effect's clock, not wall time. Identical time values need no material
mutation; zero resets the wave phases. Non-finite and negative times are ignored.
The demo updates one cached asset; the editor lazily creates one plane/material
and reuses them across toggles. There are no per-frame mesh/image creations,
extra VFX owners or water simulation ticks. `base.perceptual_roughness` controls
the resolved BRDF; unresolved slope variance is added in the shader.

`extension.sky_radiance` is an art-directed linear sky contribution reflected by
Fresnel. Set it to zero if the host provides its own environment map. It is not
a captured skybox or a source of fabricated firework images. The material uses
forward opaque rendering, and does not require transparent OIT storage.

Enable **Water reflections (experimental)** in the editor, or add
`--water-reflections` to the sea demo. This adds one mirrored HDR presentation
camera with per-view culling and billboard orientation. It draws the existing
native GPU additive sprites, flipbooks and trails, plus ordinary opaque scenery;
it never creates another effect player or advances the simulation. Transparent
smoke, CPU/readback particles, mesh/ribbon particles and scene-dependent semantic
materials are excluded. The existing main-view OIT guard and budget remain intact.

```rust,ignore
use aestra_bevy::preview::planar::PlanarReflection;
// receiver is the entity carrying MeshMaterial3d<WaterMaterial>.
commands.entity(source_camera).insert(PlanarReflection::new(receiver));
```

Registering `WaterMaterialPlugin` also installs reflection support. Use one
distinct water material per source camera. The source must be a perspective
camera above a horizontal Y-up water plane. Capture is half width/height by
default; `resolution_scale` and `target_budget_bytes` control presentation
resolution, with no effect-owner limit. The 16 MiB default budget estimates the
capture HDR output, two HDR intermediates, depth and a separate filtered HDR mip
chain, excluding view-local draw buffers and driver allocations. At 640×360 this
is 8,908,504 bytes (8.50 MiB). `PlanarReflectionStatus::snapshot()` reports
the target estimate, queued draws/omissions and configuration rejection. Queued
draws are not GPU-completion or live-particle counts.

Targets are reused, resized in place and released when the request is removed,
invalid, inactive, below water, or wholly above the water horizon. All water
receivers are excluded from the capture. The reflection has no UI, bloom, display
tonemapping, alpha sorting or OIT list. Before the main water draw, a GPU box
filter builds HDR mips with area weights that handle odd dimensions. Water
reflects the view direction about its resolved wave normals and samples twelve
GGX visible-normal directions. Exact water/air Fresnel and Smith visibility
weight the samples; solid-angle PDFs select the filtered mip level. The main
view supplies bloom/display processing once. In reflection mode,
analytical point-light specular highlights are replaced by captured radiance to
avoid counting the same luminous source twice. The initial direct-light term is
unshadowed clustered point-light diffuse; directional/spot/area and indirect
receiver lighting parity remain open. Turning reflections off restores normal PBR.

Geometry remains flat, with no displaced silhouette, foam, refraction or SSR.
This is an experimental additive reflection foundation. Smoke attenuation,
multi-resolution whole-effect energy measurements and production GPU cost
acceptance remain open. The directional lookup uses planar capture radiance:
flat normals recover the planar alignment, but this is a far-field approximation
for perturbed rays, without depth-aware intersections. Nearby barge parallax,
wave self-occlusion, finite capture coverage and smoke attenuation remain limited.
Rough-water streaks come from normal orientation and the reflection lobe; no
source-height cutoff, reflection boost or altitude fade is applied. Extinguishing
sources disappear naturally from the next capture.

The [selective planar reflection plan](../new/AESTRA_PLANAR_REFLECTIONS_PLAN.md)
tracks the staged implementation and remaining qualification gates.
