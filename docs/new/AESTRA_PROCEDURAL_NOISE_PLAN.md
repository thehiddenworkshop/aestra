# Shared procedural noise and particle sampling

Assessment: 2026-11-01. N1 scalar material fields and N2 spatial particle flow
are implemented; N3 launch sampling is implemented. The aim is reusable editor/runtime authoring, with
fireworks as a validation case rather than a separate implementation.

## What exists

- `crates/aestra-core/src/noise.rs` and `crates/aestra-runtime/src/noise.rs`:
  shared seeded 2D/3D value/gradient fields and normalized fBm. The existing typed
  function system exposes eight built-in nodes under the Noise palette category.
- `crates/aestra-gpu/src/shaders/advected_smoke.wesl`: smoke uses multi-octave
  value noise and domain distortion, including a coherent advected world field.
- `crates/aestra-gpu/src/shaders/emission_grain.wesl`: another value-noise
  implementation supplies stationary filament grain and filters distant detail.
- `crates/aestra-runtime/src/stateful.rs` and `crates/aestra-gpu/src/lib.rs`:
  legacy turbulence is seeded per particle/channel and varies with particle age.
  Its fixed temporal frequency is not an artist-controlled spatial flow field.
- `crates/aestra-core/src/velocity.rs`: constant, cone, sphere, hemisphere,
  disk and ring velocity modes exist. The explicit directional samplers use
  uniform solid angle/area where appropriate. This is separate from position
  shapes and does not provide a stratified shell-layout mode.
- The fluid extension has its own turbulence controls. Replacing that solver's
  behavior is outside the first milestone.

The main gap is shared, discoverable authoring with consistent semantics. Adding
more unrelated shader snippets would leave that gap in place.

## N1 — Shared scalar fields and material nodes

Start with seeded value noise and improved gradient noise in 2D and 3D. Expose
fBm as composition over a selected basis, rather than conflating it with the
basis algorithm. Add frequency/scale, offset, amplitude, seed, octave count,
lacunarity and gain controls, with explicit output ranges. Position is a typed
input: UV, effect/local or world coordinates are deliberate graph choices.
Material time must come from effect playback, not an independent wall clock.

Keep the serializable definitions in engine-independent core and CPU sampling in
the runtime. The implementation supplies matching WESL from the compiler's existing
built-in function catalog, avoiding an inverted compiler → GPU dependency. No new external
engine dependency or new crate is required merely to expose these algorithms.
The compiler should include only used helpers, validate domains and reflect
parameters into the existing material/editor control system. The graph palette,
node inspector, previews and thumbnails must all support the same expressions.

Use a documented integer hash/seed contract, correct flooring for negative
coordinates and deterministic per-backend evaluation. Test CPU/GPU numeric parity
with stated tolerances; do not promise universal bit-identical floating-point
results across devices. Fragment derivatives can filter unresolved octaves;
compute/vertex evaluation needs explicit LOD or an unfiltered contract, never
implicit fragment derivatives. Octave budgets must be visible/validated, not
unbounded per-pixel loops. A baked texture remains a useful lower-cost option.

Acceptance: save/reload a material with nested reusable functions; graph,
thumbnail and runtime agree; seed and coordinates survive pause/restart; negative
coordinates and distance filtering pass; native GPU cost is measured. Existing
smoke/grain defaults and validated images remain unchanged until a separate
controlled adoption comparison succeeds.

Implemented N1:

- Stable built-in IDs, typed editable defaults and automatic initial UV/world
  wiring; nested project functions and ordinary saved graph assets work.
- Exact integer-hash twins, floor/wrap semantics, normalized unsigned output,
  bounded octaves and explicit footprint filtering toward the field mean.
- Canonical built-ins evaluate in graph previews and saved-material thumbnails.
  Bounded transitive graph dependencies also work in effect thumbnails/hover;
  arbitrary edited shader source retains the background restriction.
- `sample-project/effects/noise_smoke_lab.aestra.ron` and the separate
  `assets/test/effects/fireworks_noise_smoke.aestra.ron` use the same authored
  material/function. Source particle modules/events/outputs remain unchanged.
- Native parity tests cover all eight production kernels; portable compilation
  validates WGSL, SPIR-V and HLSL. See the [authoring/API guide](../examples/procedural_noise.md)
  for reproduction, ranges and costs.

Filtering is explicit in N1: default Footprint = 0 is unfiltered. Automatic
fragment-derivative wiring and camera-distance material LOD remain follow-ups,
not claims of the current implementation. The smoke study is a reuse/compilation
qualification, not adoption or final photorealistic art approval.
The [N1 native review](../../benchmarks/fireworks/n1-procedural-noise-2026-11-01.md)
retains CPU/GPU parity and public-demo capture evidence.

## N2 — Shared spatial flow for particles

Add an opt-in spatial noise/curl module with scale, strength, evolution, seed,
coordinate space and a clear force/velocity integration contract. Preserve the
existing turbulence module's saved behavior. Sample one spatial field so nearby
smoke deposits experience correlated flow instead of unrelated per-particle kicks.
Use the same shared basis and fixed simulation tick on CPU/GPU. Gameplay owns
rocket direction, speed and fuse; VFX flow must not alter the host trajectory.

Curl is a vector field derived from a potential, not another scalar-noise basis.
For incompressible-style motion, apply varying amplitude/boundary modulation to
the potential before taking curl. Normalizing the resulting vectors or masking
them afterward does not in general preserve zero divergence. Numerical finite
differences require a scale-aware step and a measured error/cost contract;
analytic derivatives are an optimization to evaluate, not assume.

Acceptance: a saved editor smoke effect leaves a coherent drifting wake, with
pause/restart and moving-emitter/world-space tests. CPU/GPU comparison, divergence
error, finite values, bounds/culling, births, events and per-particle cost are
verified. PlaybackOnly works without adding replay storage or another clock.

Implemented N2:

- Opt-in Spatial Flow module, ordinary property payload/registry/inspector and
  undo path, with typed host configuration and offline artifact persistence.
- Shared N1 value/fBm potential, Noise/Curl modes, scale/strength/evolution/
  offset/seed/coordinate controls and a visible 1–3 octave force budget.
- CPU reference and production native fixed-tick integration use the same field,
  force ordering and existing playback clock. Legacy Turbulence remains intact.
- Constant potential normalization preserves curl rather than normalizing each
  vector. Centered differences resolve the highest octave; finite/divergence,
  full-u32 seed and mirrored/nonuniform world-transform cases are tested.
- Effect-space bounds include a conservative force excursion. World fields keep
  conservative visibility and report ForwardOnly host placement; exact historical
  root-pose reconstruction is not claimed. GPU world-flow culling bounds and
  dynamic parameter bindings remain follow-ups before broad adoption.
- Spatial Flow Wake Lab reuses the editor's N1 material. A separate matched
  fireworks control/candidate pair keeps launches, event demand and capacities.
  Native parity/cost and fixed-tick tests qualify PlaybackOnly without checkpoints.

See the [Spatial Flow guide](../examples/spatial_flow.md) for integration,
coordinate/replay limits and cost, and the
[N2 native review](../../benchmarks/fireworks/n2-spatial-flow-2026-11-01.md)
for retained qualification evidence. This is an opt-in capability/study, not
automatic adoption or final smoke-art approval.

## N3 — Deliberate particle distributions

Add uniform/stratified sampling with optional seeded jitter independently for
spawn positions and velocity directions. For shells, evaluate a stratified or
Fibonacci sphere layout with a seed-controlled rotation, then small angular,
speed and lifetime variations. Keep shape support and speed sampling explicit.

Scalar noise does not by itself produce a well-distributed set of directions;
blue-noise/Poisson sampling is also a sampling problem, not a scalar field.
Do not use unbounded rejection loops or silently drop requested births when
applying a density mask. The first implementation should preserve requested
counts through bounded jitter/warping. Density-weighted sampling needs a separate
normalized, bounded sampling policy before adding it to the UI.

Acceptance: exact requested counts, stable birth ordinal/seed mapping, sensible
behavior for tiny and changing bursts, angular/area coverage and finite bounds.
Editor and public host API expose the same saved settings; no host choreography
or new owner ceiling is introduced.

Implemented N3:

- Optional Particle Sampling module, with independent Legacy/Uniform/Stratified
  position and direction modes, a seeded Fibonacci sphere and bounded jitter.
  Existing Initialize speed/lifetime ranges retain independent random streams.
- Saved metadata/inspector/dropdowns, numeric edits/undo, host facade, compiler
  validation and offline artifact persistence use the same settings. Analytic
  effects stay analytic; the module does not allocate replay/owner storage.
- Event and host-input births carry child index/requested fan-out in versioned,
  previously unused record lanes. Each source gets its own layout, including
  simultaneous sources, changing counts and destination prefix admission. Direct
  emission uses an explicit repeating Sequence count, not a guessed burst boundary.
- Exact counts, angular coverage, area/volume density, finite support, large
  ordinals, seed/jitter, checkpoint restore, native production births, native
  helper parity/cost and analytic shape/playback tests qualify the implementation.
- Particle Sampling Lab and matched gold-shell Uniform/Stratified studies reuse
  normal saved effects. Main-star materials, ballistics, ranges and event settings
  are held fixed; downstream distance-trigger demand may change with trajectories.

V1 settings are authored constants. Direction sampling requires an explicit
Initialize velocity mode. Analytic position sampling supports all eight shapes;
stateful positions support Point/Sphere/Box and diagnose unsupported combinations.
Continuous analytic capacity-pressure parity remains an existing execution gap;
its failed native observation is kept separate from unconstrained layout parity.
See the [sampling guide](../examples/particle_sampling.md) for the exact cohort,
coordinate, compatibility and reproduction contracts, and the
[N3 native review](../../benchmarks/fireworks/n3-particle-sampling-2026-11-01.md)
for captures, measured cost and the retained admission counterexample. This adds authoring tools,
not automatic finale adoption or final shell-art approval.

## Fireworks priorities

1. N1: expose the existing smoke/grain ideas as reusable material authoring.
2. N2: coherent smoke advection, keeping deposited smoke behind the rocket.
3. N3: controlled shell spacing and variation, preserving readable radial spokes.

Worley/cellular noise, ridged variants, richer domain warping and baked volume
fields can follow a demonstrated authoring need and cost measurement. More
algorithms alone will not fix ballistic motion, burn/cooling, trail persistence,
exposure or water reflections; those remain independent visual requirements.

References: Ken Perlin's [improved-noise reference](https://mrl.cs.nyu.edu/~perlin/noise/)
and Bridson, Hourihan and Nordenstam's
[Curl-Noise for Procedural Fluid Flow](https://www.cs.ubc.ca/~rbridson/docs/bridson-siggraph2007-curlnoise.pdf).
