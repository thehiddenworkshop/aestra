# Spatial particle flow

Add **Spatial Flow** to an emitter's **Particle Update** stack. This is an opt-in
fixed-tick acceleration field shared by nearby particles. It uses N1's seeded 3D
value/fBm basis, independent of spawn ordinal, particle age and the effect's
particle-random seed. Existing Turbulence retains its saved behavior.

| Control | Default | Meaning |
|---|---|---|
| Mode | curl | `noise`: three signed scalar fields; `curl`: curl of a vector potential |
| Space | effect | `effect`: coordinates follow the effect; `world`: field samples host-world coordinates |
| Scale | 0.15 | Spatial frequency, 0.001–64 |
| Strength | 1 | Acceleration bound per selected-space axis, 0–1000 units/s² |
| Evolution | `[0.12, 0.04, 0.08]` | Potential translation in selected-space units per effect second |
| Offset | zero | Coordinate offset before Scale, each component within ±65,536 |
| Seed | 17 | A full `u32` field seed, independent of particle identity |
| Octaves | 1 | 1–3 normalized fBm octaves; fixed lacunarity 2 and gain 0.5 |

Evolution components also lie within ±65,536. Invalid/nonfinite settings are
rejected. Strength zero stops the force while retaining the module's stateful
classification. Configuration uses normal saved module properties, inspector
controls and undo/redo. V1 settings are authored constants: dynamic effect/host
parameter bindings are diagnosed rather than silently ignored.

Spatial Flow automatically requests persistent simulation. Each 1/60-second
tick evaluates the field at the particle's previous position and the existing
effect tick time. Acceleration adds to Motion's gravity and legacy turbulence;
velocity is then damped by drag, position advances, and existing collision/event
processing runs. Gameplay's projectile direction, velocity and fuse remain host
inputs; attach this module to its smoke/VFX emitter to perturb smoke independently.
Opting an analytic emitter into persistent simulation also selects the existing
stateful Motion integrator, as Collision/Homing do.

## Curl contract and cost

Coordinates are `(position + offset - evolution * effect_time) * scale`.
Curl takes centered differences of three independently seeded potentials, using
a step of `1 / (128 * highest_octave_frequency)` in noise coordinates. Its
constant normalization is based on the quintic value basis's derivative bound
and octave weights. This bounds each acceleration component by Strength without
normalizing every vector. Strength changes amplitude without making Scale an
unintended acceleration multiplier.

This is a curl acceleration field, not a fluid solver. Any future spatially
varying amplitude/boundary mask must modulate the potential before curl; masking
or normalizing the resulting vectors generally introduces divergence. The current
Strength is spatially constant. CPU tests check discrete divergence against 0.003
in the supplied fixtures, continuity, finite values and restart determinism.

Noise uses three scalar samples per particle/tick; Curl uses twelve. Each scalar
octave interpolates eight corners. Three octaves are an explicit force-work
budget, separate from N1's material octave budget. No owner-count ceiling or
per-particle replay allocation is added. The existing tick-parameter block grows
by thirteen 32-bit words; persistent particle stride and storage binding count
remain unchanged.

On the retained RTX 4070 SUPER / Vulkan microbenchmark, 4,096 World samples took
median 0.0202 ms for one-octave Curl and 0.0549 ms for three octaves, over eight
dispatches after warmup. These measure force evaluation only: integration,
upload, simulation scheduling, transparency and rendering cost are additional.
See the [native review](../../benchmarks/fireworks/n2-spatial-flow-2026-11-01.md).

## Coordinates, placement and replay

World mode converts effect-space particle positions through the current host
effect-to-world affine, then converts world acceleration through its inverse
linear part. Rotation, mirrored transforms and nonuniform scale are supported.
A singular pose contributes zero flow on the GPU; the CPU pose setter rejects
invalid/noninvertible poses. Large worlds should rebase before
sampling; 32-bit coordinates eventually lose subcell detail.

World is the field's coordinate space, not a change to the effect's simulation
storage space. Leave the effect root stationary and move the source/emitter or
its binding to deposit smoke behind a moving rocket. Moving the whole effect
root still follows the existing particle-placement policy. The included wake lab
uses distance-linked deposits with zero inherited launch velocity.

World mode reads live host placement, which the binding trace does not record.
It therefore reports **ForwardOnly**, even with a binding trace; exact backward
replay of an unrecorded moving root is not advertised. Effect mode retains the
existing replay policy. PlaybackOnly adds no checkpoints or extra clock. Native
tests explicitly verify this, pause behavior, moving deposits and CPU/GPU
trajectories. A CPU replay caller can provide historical poses explicitly.

Evolution follows each effect's local clock. To phase-align a host-wide evolving
field across instances, set `offset = base_offset - evolution * global_spawn_time`
for each instance, or use zero Evolution for a stationary shared field.
Use a rebased gameplay clock and offsets within the authored numeric range.

Effect-space culling bounds include the force's conservative lifetime excursion
after the emitter transform. World flow retains conservative visibility because
an arbitrary live inverse scale cannot be bounded by the static artifact.
Mesh/ribbon systems must not reenable static culling for those draws. GPU-derived
world-flow bounds are a performance follow-up before large-show adoption.

## Try it

Open **Spatial Flow Wake Lab** in `sample-project`. It uses the editable N1 noise
smoke material and shows deposited smoke drifting after its moving source dies.
Edit Spatial Flow on **Deposited smoke**, or switch Mode/Space through the normal
Feathers dropdowns. The source's rocket motion and distance-emission link remain
independent.

The public fireworks host loads a separate matched control/candidate pair:

```powershell
cargo run --release --locked -p aestra-demo-fireworks --bin fireworks -- --effect effects/fireworks_spatial_flow.aestra.ron --star-lights --no-audio
# Control: effects/fireworks_spatial_flow_control.aestra.ron
```

Both studies use identical launches, lifetimes, capacities, events and renderers.
Only smoke gets the flow module; their legacy smoke turbulence is zeroed in both
copies to isolate this comparison. Existing repertoire assets are not rewritten.
Regenerate these study files with `author_spatial_flow_study` in the same package.

Hosts author the same semantic module, then use the ordinary project compiler
or offline artifact path:

```rust
use aestra_bevy::{ModuleInstance, SpatialFlow, FlowMode, FlowSpace};
let flow = SpatialFlow {
    mode: FlowMode::Curl,
    space: FlowSpace::World,
    strength: 2.0,
    ..Default::default()
};
smoke_emitter.modules.push(ModuleInstance::spatial_flow(flow));
```

CPU tools can call `aestra_runtime::spatial_flow::{sample, acceleration}` or
`StatefulSimulation::set_spatial_flow` and `set_flow_world_from_effect`. All use
the same seed, coordinate, fixed-tick and force contracts as native playback.
