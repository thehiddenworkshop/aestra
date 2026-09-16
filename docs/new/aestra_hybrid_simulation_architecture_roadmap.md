# Aestra Hybrid Simulation Architecture

**Status:** Proposed architecture and implementation roadmap  
**Reviewed against:** `main` on 2026-09-10  
**Repository:** https://github.com/thehiddenworkshop/aestra

---

## 1. Executive decision

Aestra should **not replace its current analytic/stateless particle simulation with a conventional fully stateful runtime**.

Instead, Aestra should evolve into a **hybrid simulation engine**:

> **Analytic whenever possible, stateful whenever necessary, staged when synchronization or iterative solvers require it.**

The compiler should choose the weakest execution model that preserves the semantics of each simulation dependency island.

The target architecture is:

```text
                         Aestra Effect
                              │
                              ▼
                       Semantic authoring
                              │
                              ▼
                         Compiler / IR
                              │
                  dependency + requirement analysis
                              │
               ┌──────────────┼──────────────┐
               ▼              ▼              ▼
           ANALYTIC        STATEFUL         STAGED
        absolute-time      persistent      multi-pass /
          evaluation         state         iterative state
               │              │              │
               └──────────────┼──────────────┘
                              ▼
                 coherent presentation snapshot
                              │
                              ▼
                           Renderer
```

This is an evolution of Aestra's current architecture rather than a rewrite.

Several important pieces already exist:

- deterministic absolute-time CPU and GPU particle evaluation;
- a fixed-step `PlaybackClock`;
- generic seek/checkpoint planning in `aestra-runtime`;
- GPU-resident trail checkpoints and replay;
- compiler particle-attribute liveness;
- a stable 48-byte GPU presentation particle ABI;
- a benchmark suite specifically designed to expose capacity-bound analytic simulation;
- authoring concepts for custom simulation domains and simulation stages.

The main missing layer is **compiler/runtime representation of mixed simulation execution classes**.

---

# 2. Terminology

The existing behavior is better described as **analytic simulation** or **absolute-time simulation** than "stateless rendering".

Rendering and simulation are separate concerns.

Aestra currently largely computes:

```text
particle_state = f(
    effect_time,
    particle_index,
    deterministic_seed,
    emitter_parameters
)
```

instead of:

```text
state[n + 1] = update(state[n], dt)
```

The renderer then consumes the result.

Use these terms in the architecture:

### Analytic simulation

State can be reconstructed directly for an arbitrary time.

```text
state(t) = f(seed, parameters, t)
```

No previous simulation tick is required.

### Stateful simulation

The next state depends on the previous state.

```text
state[n + 1] = f(state[n], inputs[n], dt)
```

### Staged simulation

Stateful computation that additionally requires ordered GPU passes, synchronization, neighborhood access, iterative solving, grids, or other shared resources.

```text
stage A
  ↓
barrier
  ↓
stage B
  ↓
barrier
  ↓
stage C × N iterations
```

### Presentation state

Renderer-facing data such as:

```text
position
size
rotation
color
normalized age
alive
particle identity
```

Presentation state must remain separate from simulation-only data.

### Simulation state

Persistent data required to compute the next simulation step:

```text
position
velocity
age
collision state
constraint state
grid velocity
pressure
density
solver scratch
...
```

---

# 3. Current Aestra architecture relevant to this change

The repository is already structured well for the hybrid approach.

The important flow documented in `docs/ARCHITECTURE.md` is:

```text
EffectAsset
    │
    ▼
aestra-compiler
    │
    ▼
CompiledEffect / aestra-runtime
    ├── CPU reference
    │
    └── aestra-gpu
            │
            ▼
     aestra-bevy-render
```

The hybrid simulation work should preserve those boundaries.

Relevant current files:

- [`docs/ARCHITECTURE.md`](https://github.com/thehiddenworkshop/aestra/blob/main/docs/ARCHITECTURE.md)
- [`crates/aestra-core/src/model.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-core/src/model.rs)
- [`crates/aestra-compiler/src/lib.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-compiler/src/lib.rs)
- [`crates/aestra-runtime/src/lib.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-runtime/src/lib.rs)
- [`crates/aestra-runtime/src/checkpoint.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-runtime/src/checkpoint.rs)
- [`crates/aestra-artifact/src/lib.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-artifact/src/lib.rs)
- [`crates/aestra-gpu/src/lib.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-gpu/src/lib.rs)
- [`crates/aestra-gpu/src/shaders/aestra_simulation.wesl`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-gpu/src/shaders/aestra_simulation.wesl)
- [`crates/aestra-bevy-render/src/gpu.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-bevy-render/src/gpu.rs)
- [`crates/aestra-bevy-render/src/gpu/trail_checkpoints.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-bevy-render/src/gpu/trail_checkpoints.rs)
- [`crates/aestra-bevy-render/src/gpu/trail_replay.rs`](https://github.com/thehiddenworkshop/aestra/blob/main/crates/aestra-bevy-render/src/gpu/trail_replay.rs)
- [`apps/aestra-bench`](https://github.com/thehiddenworkshop/aestra/tree/main/apps/aestra-bench)
- [`docs/aestra_runtime_benchmarking_profiling_plan.md`](https://github.com/thehiddenworkshop/aestra/blob/main/docs/aestra_runtime_benchmarking_profiling_plan.md)

---

# 4. What already exists

## 4.1 Analytic GPU simulation is fully real, not just a design intention

`aestra_simulation.wesl` currently receives absolute effect time through `GpuGlobals.time`.

For each slot it reconstructs such values as:

- emitter ownership;
- cycle;
- particle identity;
- emission count;
- spawn time;
- age;
- lifetime;
- initial direction;
- speed;
- shape origin;
- drag;
- gravity;
- turbulence;
- rotation;
- color;
- size.

For example, motion is currently reconstructed analytically from age:

```text
travel = function(speed, drag, age)

position =
    origin
    + direction * travel
    + gravity * age² * 0.5
    + turbulence(age)
```

There is no persistent `velocity` read from the previous frame in the current main particle simulation shader.

This is the core of Aestra's direct-seek behavior.

---

## 4.2 The shader currently dispatches by particle capacity

`aestra-bevy-render` currently computes:

```rust
workgroups = artifact.total_slots.div_ceil(WORKGROUP_SIZE)
```

and the simulation shader receives:

```text
global_invocation_id.x → slot
```

then rejects slots that are not currently alive.

Therefore the current analytic GPU simulation is substantially **capacity-driven**.

Conceptually:

```text
cost ≈ total configured slots × reconstruction work
```

rather than:

```text
cost ≈ currently alive particles × update work
```

for all workloads.

This matters especially for sparse effects.

Aestra already recognizes this in its benchmark strategy:

```text
B004 — Sparse Large Capacity
500k capacity
1% occupancy

Purpose:
expose capacity-bound analytical simulation
```

That benchmark should remain central to decisions about the analytic backend.

---

## 4.3 A generic checkpoint contract already exists

`aestra-runtime/src/checkpoint.rs` already defines:

```rust
pub enum SimulationSeekMode {
    StatelessDirect,
    CheckpointRestore,
    RestartReplay,
}
```

and a bounded:

```rust
CheckpointStore<T>
```

with:

- checkpoint cadence;
- maximum entries;
- maximum bytes;
- revision/seed/backend context;
- nearest checkpoint lookup;
- invalidation;
- seek planning.

The seek planner already implements the desired basic rule:

```text
analytic
    → direct target

stateful + forward seek
    → replay from current state

stateful + checkpoint + backward seek
    → restore closest checkpoint <= target
      + replay forward

stateful without checkpoint
    → restart
      + replay forward
```

This is exactly the foundation needed for stateful particles.

It should be generalized rather than replaced.

---

## 4.4 GPU-resident checkpoints already exist for trails

Trail history has already forced Aestra to solve a history-dependent problem.

`trail_checkpoints.rs` explicitly uses:

> GPU-resident snapshots; no readback and no serialized device state.

Mutable GPU buffers are copied into checkpoint GPU buffers and copied back during restore.

This is the right model for future stateful GPU simulation too:

```text
GPU simulation state
        │
        ├── GPU → GPU checkpoint copy
        │
        └── restore with GPU → GPU copy
```

Do **not** design normal editor seeking around GPU → CPU → GPU readback.

Trail replay also already advances through canonical 60 Hz observations and bounds long replay work per rendered frame.

This existing implementation should inform the generic stateful simulation checkpoint backend.

---

## 4.5 Aestra already owns a canonical fixed-step clock

`aestra-runtime` currently defines:

```rust
pub const DEFAULT_PLAYBACK_TICK_RATE: u32 = 60;
```

and a `PlaybackClock` shared by editor, viewer, and game playback.

This is important.

Stateful simulation should be built on top of the same canonical simulation clock rather than introducing a second clock inside the GPU backend.

---

## 4.6 Particle liveness analysis already exists

The runtime already represents:

```rust
pub struct ParticleLayout {
    pub attributes: Vec<ParticleAttribute>,
    pub transient_attributes: Vec<ParticleAttribute>,
}
```

and compiler liveness decides which attributes survive.

Current attributes already include:

```text
Position
Velocity
Age
Lifetime
NormalizedAge
Rotation
AngularVelocity
Size
Color
```

This gives Aestra a useful starting point for persistent state analysis.

However, future work should explicitly distinguish:

```text
persistent simulation attributes
transient simulation attributes
presentation attributes
```

rather than treating all live particle attributes as one storage category.

---

## 4.7 The GPU presentation ABI is already correctly separated

`GpuParticle` is explicitly documented as a stable **48-byte storage/readback ABI** containing only live presentation state.

It currently contains:

```text
color
position
size
rotation
normalized_age
packed emitter/alive
particle index
```

Ribbon/trail scratch already lives in a separate auxiliary buffer.

This is exactly the design principle to keep.

Do **not** turn `GpuParticle` into a giant simulation struct containing:

```text
velocity
collision data
fluid attributes
neighbor state
constraint state
...
```

Instead introduce separate simulation-state buffers.

---

## 4.8 Compiler metadata is already close to what is needed

`ModuleMetadata` currently contains:

```rust
stages
inputs
reads
writes
tags
capabilities
approximate_cost
```

This is the natural place to add **semantic simulation requirements**.

The current compiler already performs:

- module validation;
- stage validation;
- attribute read/write analysis;
- liveness;
- typed lowering;
- source mapping.

The hybrid simulation classifier belongs here.

---

## 4.9 The authoring model already anticipates broader simulation

`aestra-core` currently has:

```rust
pub enum SimulationDomain {
    Particle,
    Strip,
    Custom(String),
}
```

and:

```rust
pub enum StageKind {
    EffectSpawn,
    EffectUpdate,
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Simulation(String),
}
```

This is useful, but two concepts must not be conflated:

```text
SimulationDomain
    = what topology/data domain is simulated

SimulationClass
    = how temporal execution works
```

For example:

```text
Particle domain + Analytic
Particle domain + Stateful
Custom("fluid-grid-3d") + Staged
Strip domain + Stateful
```

`SimulationDomain` should therefore **not** be repurposed as the analytic/stateful selector.

---

## 4.10 Collision has already been anticipated semantically

`EventTrigger` already contains:

```rust
OnCollision
```

although Aestra does not yet have a general collision simulation module.

This is useful because collision-generated events can eventually reuse the choreography/event architecture, but collision itself still requires the new simulation execution support described here.

---

# 5. Current architectural gaps

The current code has several important gaps before stateful collision or fluids can be added cleanly.

## 5.1 `CompiledEffect.seek_mode` is effect-wide

`CompiledEffect` currently stores one:

```rust
seek_mode: SimulationSeekMode
```

for the complete effect.

That is sufficient while all particle simulation is analytic.

It is too coarse for:

```text
Explosion
├─ flash          analytic
├─ sparks         analytic
├─ debris         stateful
└─ smoke fluid    staged
```

Only the debris/fluid portions should require replay.

The analytic portions should remain directly seekable.

---

## 5.2 The compiler currently hard-codes `StatelessDirect`

Even though `SimulationSeekMode` already supports stateful behavior, the compiler currently constructs every compiled effect with:

```rust
seek_mode: SimulationSeekMode::StatelessDirect
```

There is not yet any semantic derivation of statefulness.

This is the most immediate compiler gap.

---

## 5.3 `SimulationSeekMode` currently mixes two different concepts

The current enum combines:

1. **effect semantics**
   - can this simulation be evaluated directly?
   - does it depend on history?

2. **backend capability**
   - can this backend snapshot its state?

Those should be separated.

For example, the semantic compiler can know:

```text
this island requires replay
```

but it cannot universally know:

```text
this backend can create checkpoints
```

because:

- CPU state can usually be cloned;
- a GPU backend may support GPU-resident snapshots;
- another backend may only support restart/replay;
- a future external engine integration may expose different capabilities.

Recommended split:

```rust
enum TemporalSemantics {
    Direct,
    HistoryDependent,
}

enum SimulationClass {
    Analytic,
    Stateful,
    Staged,
}

struct BackendSimulationCapabilities {
    checkpoints: bool,
    staged_dispatch: bool,
    ...
}
```

Then at runtime:

```text
Direct
    → StatelessDirect

HistoryDependent + checkpoints supported
    → CheckpointRestore

HistoryDependent + checkpoints unavailable
    → RestartReplay
```

The current `SimulationSeekMode` can remain as the **resolved runtime strategy**.

It should stop being the sole semantic description persisted by the compiler.

---

## 5.4 Current runtime stages are not yet generic simulation passes

`ExecutionPlan` currently has:

```rust
emitter_update
particle_spawn
particle_update
```

and `RuntimeStage` has:

```text
EmitterUpdate
ParticleSpawn
ParticleUpdate
```

These are useful semantic lifecycle stages.

They are not sufficient to represent something like:

```text
fluid.inject
fluid.advect_velocity
fluid.advect_density
fluid.divergence
fluid.pressure × 20
fluid.project
```

The existing authoring `StageKind::Simulation(String)` is a useful hook, but it currently does not have a matching generic compiled multi-pass execution model.

Do not overload ordinary `ParticleUpdate` with fluid passes.

Add an explicit compiled staged-simulation plan.

---

## 5.5 Persistent physics state does not exist yet

Current GPU particles are reconstructed presentation records.

There is no generic persistent simulation buffer containing previous:

```text
position
velocity
age
collision state
...
```

The stateful backend needs separate storage.

---

## 5.6 Checkpoint context will eventually need stronger instance/input identity

Current `CheckpointContext` identifies checkpoints with:

```text
effect
revision
seed
backend
```

That is a good start.

For stateful nested effects and external scene interactions, checkpoint validity may also depend on:

```text
instance identity / clip path
external input epoch
collision scene revision
simulation quality/profile
state layout version
```

Two instances of the same effect with the same seed are not necessarily interchangeable if they collide with different world geometry.

Checkpoint identity should therefore become explicitly capable of representing simulation inputs, not only effect identity.

---

# 6. Why keep analytic simulation?

Aestra should keep analytic execution because it provides properties that a conventional stateful runtime cannot cheaply reproduce.

It is not because analytic simulation is always faster.

---

## 6.1 Random-access time

For an analytic island:

```text
state = f(seed, parameters, time)
```

Aestra can jump:

```text
2.0 s → 48.7 s
```

without evaluating the interval.

This is extremely valuable for Aestra's timeline-oriented editor.

It gives:

- direct seeking;
- responsive scrubbing;
- easy backwards timeline movement;
- instant arbitrary-time thumbnails;
- no simulation warm-up;
- simple timeline comparison.

---

## 6.2 Parameter edits can update arbitrary time immediately

Suppose the playhead is at:

```text
8.4 s
```

and the artist changes gravity.

Analytic:

```text
evaluate(new_gravity, 8.4)
```

Stateful:

```text
invalidate history
restore/restart
replay to 8.4
```

For authoring, analytic evaluation is much more interactive.

This is particularly valuable for AI-assisted authoring, where many parameter variants may be evaluated at representative timestamps.

---

## 6.3 Determinism is straightforward

Given:

```text
asset
seed
time
```

the result can be reconstructed.

This is useful for:

- tests;
- visual regression;
- deterministic capture;
- replays;
- multiplayer VFX reconstruction;
- debugging;
- AI evaluation.

---

## 6.4 No simulation debt after culling

An analytic effect can be completely skipped while invisible.

When visible again:

```text
evaluate(current_time)
```

There is no requirement to simulate all missing frames.

This enables aggressive runtime culling policies.

For example:

```text
visible
    → evaluate every presentation

far away
    → evaluate at reduced frequency

culled
    → do not simulate

visible again
    → directly reconstruct current time
```

Stateful systems cannot generally do this without either continuing to simulate or paying catch-up cost.

---

## 6.5 No warm-up

Looping ambient effects can be opened directly at a mature state.

Examples:

- fire;
- rain;
- waterfall spray;
- ambient dust;
- looping magical fields.

A stateful system may need to simulate seconds of warm-up.

Analytic systems do not.

---

## 6.6 Lower persistent simulation-state memory

Large analytic particle systems do not need persistent previous-frame state for every particle.

This can reduce VRAM and checkpoint memory substantially.

Example:

```text
800,000 sparks       analytic
30,000 smoke motes   analytic
5,000 debris         stateful
128³ fluid grid      staged
```

Only the 5,000 debris particles and fluid grid need historical simulation state.

The 830,000 analytic particles do not need to be checkpointed.

This is one of the strongest arguments for hybrid simulation.

---

## 6.7 Trade memory bandwidth for ALU

Stateful simulation generally performs persistent state reads/writes each tick.

Analytic simulation can perform more computation but less state traffic.

This gives Aestra a potentially useful GPU trade:

> **ALU instead of persistent memory bandwidth.**

Whether that wins depends on the effect.

That is why both paths should exist and be benchmarked.

---

## 6.8 Time can be parallelized for baking/testing

Analytic timestamps are independent.

Aestra can evaluate:

```text
worker A → frame 0..100
worker B → frame 101..200
worker C → frame 201..300
```

without serial simulation dependencies.

This is valuable for:

- flipbook baking;
- thumbnail generation;
- regression capture;
- effect-search previews;
- offline analysis.

---

# 7. Where analytic simulation loses

Analytic simulation has real disadvantages.

## 7.1 Repeated reconstruction

The current GPU shader repeatedly reconstructs such values as:

- spawn time;
- lifetime;
- random initial direction;
- speed;
- shape origin;
- motion terms.

A stateful system computes many of those once at spawn.

The current inverse-emission lookup table already demonstrates that Aestra has needed to optimize this reconstruction path.

---

## 7.2 Capacity-bound work

Current GPU simulation dispatches across `total_slots`.

Sparse effects therefore risk doing substantial work just to discover that most slots are dead.

This is exactly what `B004` is intended to measure.

---

## 7.3 History-dependent interactions do not fit

Examples:

```text
particle bounced last frame
particle accumulated impulse
particle is sliding
particle touched another particle
constraint correction from previous iteration
fluid velocity changed previous position
```

These are naturally stateful.

Trying to reconstruct a complete collision/interaction history from birth on every rendered frame would defeat the purpose of the analytic model.

---

# 8. Collision

Collision should be treated as the first major stateful simulation feature.

Some trivial collision can remain analytic:

```text
kill at intersection with known plane
single analytically solvable impact
simple static primitives in restricted cases
```

But general collision is history-dependent.

Example:

```text
spawn
  ↓
fall
  ↓
hit wall
  ↓
reflect velocity
  ↓
hit floor
  ↓
apply friction
  ↓
slide
  ↓
hit moving collider
```

The final state depends on the collision sequence.

Therefore:

| Collision feature | Recommended execution |
|---|---|
| Analytic kill plane | Analytic |
| Restricted single static impact | Analytic optional |
| Static primitive bounce | Stateful |
| Multiple bounce | Stateful |
| Friction / sliding | Stateful |
| Dynamic collider | Stateful |
| Scene collision | Stateful |
| Particle-particle collision | Stateful / staged |
| Neighbor constraints | Staged |

Do not make analytic collision a prerequisite for the general collision system.

A simple portable **stateful primitive collider** is a better first correctness target for the new backend.

---

# 9. Fluids

Procedural "fluid-looking" particles can remain analytic:

```text
curl noise
procedural flow field
animated turbulence
```

But genuine fluids require persistent state and multiple passes.

A simple grid fluid requires something resembling:

```text
velocity[n]
    ↓
advection
    ↓
forces
    ↓
divergence
    ↓
pressure solve × N
    ↓
projection
    ↓
velocity[n + 1]
```

Therefore real fluids belong to the **staged** backend.

Potential future simulation techniques include:

- Eulerian grid smoke;
- SPH;
- PBF;
- PIC;
- FLIP;
- MPM.

Do not select all of them now.

The first goal should be to build a generic staged execution infrastructure and prove it with a small 2D grid simulation before committing to a large fluid feature set.

---

# 10. Simulation class and seek strategy must remain separate

Recommended compiled semantics:

```rust
pub enum SimulationClass {
    Analytic,
    Stateful,
    Staged,
}

pub enum TemporalSemantics {
    Direct,
    HistoryDependent,
}
```

Recommended runtime-resolved strategy:

```rust
pub enum SimulationSeekMode {
    StatelessDirect,
    CheckpointRestore,
    RestartReplay,
}
```

The relationship is approximately:

```text
Analytic
    temporal = Direct
    seek = StatelessDirect

Stateful
    temporal = HistoryDependent
    seek =
        CheckpointRestore if backend supports snapshots
        RestartReplay otherwise

Staged
    temporal = usually HistoryDependent
    seek =
        CheckpointRestore if backend supports snapshots
        RestartReplay otherwise
```

This prevents a compiled semantic artifact from making assumptions about a specific GPU/backend checkpoint implementation.

---

# 11. Simulation requirements should live in module metadata

Do not ask artists to manually choose:

```text
Simulation Mode: Stateful
```

for ordinary effects.

Modules should declare what they require.

For example:

```rust
pub struct SimulationRequirements {
    pub temporal: TemporalRequirement,
    pub synchronization: SynchronizationRequirement,
    pub neighborhood: NeighborhoodRequirement,
}
```

Possible values:

```rust
pub enum TemporalRequirement {
    Direct,
    PreviousState,
}

pub enum SynchronizationRequirement {
    None,
    OrderedPass,
    Iterative,
}

pub enum NeighborhoodRequirement {
    None,
    Particles,
    Grid,
}
```

The compiler can derive a convenient backend class:

```text
Direct + no synchronization
    → Analytic

PreviousState + no multi-pass requirement
    → Stateful

ordered/iterative/neighborhood/grid
    → Staged
```

This is more future-proof than attaching a single hardcoded class to every module.

For UI/debugging, expose the **derived class**.

---

# 12. Simulation islands

A complete effect should be allowed to contain several execution classes.

Example:

```text
ExplosionEffect
├─ Flash                 ANALYTIC
├─ Sparks                ANALYTIC
├─ Debris                STATEFUL
├─ Smoke wisps           ANALYTIC
└─ Ground smoke          STAGED
```

The compiler should group dependent simulation work into **simulation islands**.

A conceptual compiled representation:

```rust
pub struct CompiledSimulationIsland {
    pub id: SimulationIslandId,
    pub class: SimulationClass,
    pub temporal: TemporalSemantics,

    pub emitters: Vec<usize>,
    pub dependencies: Vec<SimulationIslandId>,

    pub persistent_layout: SimulationStateLayout,
    pub transient_layout: SimulationScratchLayout,

    pub plan: SimulationPlan,
}
```

Do not make this exact struct an API requirement; the point is the separation of responsibilities.

---

# 13. Initial island granularity can follow current compiled emitters

Current `CompiledEmitter` already represents a specific emitter **region**:

```text
source emitter
region
start time
source offset
duration
seed index
execution plan
renderers
```

That gives a practical migration path.

Initial implementation:

```text
one compiled emitter-region
    ≈ one candidate simulation island
```

Then add explicit dependency edges and merging/promotion only where needed.

This avoids rewriting the authoring model before the execution infrastructure exists.

Later, islands can become more sophisticated when:

- event dependencies;
- shared simulation resources;
- particle interaction;
- fluid injection;
- constraints;
- cross-emitter data

require it.

---

# 14. Dependency promotion

Simulation requirements propagate through dependencies.

Example:

```text
Gravity
Drag
ColorOverLife
        │
        ▼
     ANALYTIC
```

Add collision:

```text
Gravity
Collision
Bounce
        │
        ▼
     STATEFUL
```

Add neighbor constraints:

```text
Particles
NeighborQuery
ConstraintSolve
        │
        ▼
      STAGED
```

General rule:

```text
Analytic → Stateful input
```

can remain hybrid when the analytic system is merely a deterministic input.

Example:

```text
analytic attractor
       ↓
stateful particles
```

During replay, evaluate the attractor at every fixed tick.

However:

```text
stateful result
       ↓
changes analytic particle trajectory
```

means that trajectory is no longer truly analytic.

The downstream island must be promoted.

Example:

```text
fluid velocity
       ↓
moves particles
```

Those particles must become stateful unless their response can genuinely be expressed as a direct closed-form evaluation.

---

# 15. One effect clock

The effect must have **one logical timeline** regardless of execution class.

If the requested time is:

```text
t = 7.35 s
```

then:

```text
Analytic island
    → evaluate at 7.35

Stateful island
    → restore checkpoint
      + replay fixed ticks
      + resolve presentation at 7.35

Fluid island
    → restore grid
      + replay stages
      + resolve presentation at 7.35
```

Only then should the complete frame be presented.

Never intentionally display:

```text
sparks = 7.35
debris = 7.00
fluid  = 6.50
```

as the final exact frame.

A checkpoint is an **internal acceleration structure**, not a visible timeline state.

---

# 16. Moving backward

Do not attempt to run stateful simulation with negative `dt`.

For a backward seek:

```text
current = 8.0
target  = 5.37
```

perform:

```text
find nearest compatible checkpoint <= 5.37
                 │
                 ▼
              restore
                 │
                 ▼
        simulate FORWARD
                 │
                 ▼
               5.37
```

If no checkpoint exists:

```text
restart
  ↓
replay forward
```

This matches Aestra's existing `CheckpointStore::plan_seek` semantics.

---

# 17. Smooth playback vs fixed simulation ticks

The existing 60 Hz canonical clock should remain authoritative for stateful simulation.

However, simulation time and presentation time need not be identical concepts.

Recommended model:

```text
SimulationFrame
    integer fixed tick

PresentationTime
    continuous editor/render time
```

For example at a 144 Hz display:

```text
stateful simulation
    state[n]
    state[n + 1]
          │
          └── interpolate presentation using alpha

analytic simulation
    evaluate directly at presentation time
```

Conceptually:

```text
presentation_time = tick_time + alpha * fixed_dt
```

Both paths then present the same logical time:

```text
analytic
    f(presentation_time)

stateful
    interpolate(state[n], state[n + 1], alpha)
```

This prevents the visual impression that analytic particles are smooth while stateful particles advance in 60 Hz jumps.

The interpolation is presentation-only and must not feed back into simulation.

For collision discontinuities, interpolation may need collision-aware correction or may be disabled for selected attributes.

---

# 18. Exact seeking vs interactive preview seeking

A heavy fluid simulation cannot necessarily reconstruct every arbitrary cursor position while the user drags the timeline rapidly.

Aestra should therefore support two evaluation qualities.

## Exact

Used for:

- released playhead;
- paused final view;
- capture;
- export;
- tests;
- authoritative playback state.

Requirements:

```text
all islands resolved to the same requested logical time
```

## Preview

Used during rapid scrubbing.

Allowed approximations:

### Stateful particles

- larger preview timestep;
- lower particle count;
- simplified collision;
- fewer interaction iterations.

### Fluids

- lower grid resolution;
- fewer pressure iterations;
- reduced checkpoint quality;
- proxy simulation.

### Analytic islands

Usually remain direct/exact because they are cheap to seek.

But **preview quality must not mean different visible timeline times**.

Prefer an approximate state representing the requested time rather than an exact old state representing the wrong time.

When scrubbing stops:

```text
preview target
    ↓
exact reconstruction
    ↓
authoritative frame
```

---

# 19. GPU checkpoints for stateful particles

Generalize the trail precedent.

Recommended GPU model:

```text
persistent simulation buffers
        │
        ├── copy → checkpoint buffer A
        ├── copy → checkpoint buffer B
        └── copy → checkpoint buffer C
```

On seek:

```text
checkpoint buffer
        │
        └── GPU copy → live simulation buffers
                          │
                          ▼
                     replay ticks
```

Keep checkpoint state on the GPU whenever possible.

The generic runtime can own:

- context;
- target selection;
- cadence policy;
- byte budget;
- invalidation rules;
- seek plan.

The backend should own:

- actual buffer handles;
- snapshot copy commands;
- restore commands;
- resource lifetime.

The existing generic `CheckpointStore<T>` can hold backend-specific opaque handles if useful, but `aestra-runtime` must not acquire Bevy/WGPU dependencies.

---

# 20. Checkpoint invalidation

Stateful checkpoints must be invalidated whenever an input that affects simulation history changes.

Examples:

```text
effect revision
seed
state layout
simulation quality
collision configuration
runtime parameter affecting simulation
host transform history
external scene/collider history
fluid resolution
solver iteration policy
```

Do not invalidate for a pure renderer-only change when simulation state is unaffected.

This suggests eventually computing a:

```text
SimulationContextFingerprint
```

from only history-relevant inputs.

That will be more efficient and safer than manually maintaining many independent invalidation flags.

---

# 21. External scene inputs are a special problem

Collision with a dynamic game scene means deterministic replay depends on the **history of the colliders**, not only particle history.

Example:

```text
particle @ frame 200
```

cannot be reproduced correctly if the wall it hit at frame 120 has moved and Aestra only knows the wall's transform at frame 200.

Therefore stateful scene simulation needs an explicit external-input contract.

Possible future abstraction:

```rust
trait SimulationInputProvider {
    fn sample_frame(&self, frame: SimulationFrame) -> SimulationInputs;
}
```

For exact backwards seeking, external inputs must be one of:

1. analytically time-addressable;
2. recorded;
3. checkpointed;
4. provided by the host as historical data.

If none is possible, the backend should explicitly report reduced seek support.

Do not silently pretend dynamic scene collision is exactly replayable.

For editor authoring, start with Aestra-owned static or animated preview colliders whose transforms are time-addressable.

This gives deterministic collision preview without requiring a complete game-world history system.

---

# 22. Nested effects and checkpoint identity

Aestra already supports reusable child `EffectClip`s and stable per-instance timing/seeds.

Stateful simulation means checkpoint identity must remain safe when multiple child instances exist.

Eventually the checkpoint context should be capable of distinguishing:

```text
effect identity
effect revision
instance / clip path
seed
backend
simulation layout
external input context
```

Do not assume:

```text
same EffectId + same seed
```

always means interchangeable state.

This becomes especially important when child effects interact with different world-space colliders.

---

# 23. Preserve the 48-byte presentation ABI

This should be a hard architectural rule unless profiling demonstrates that the presentation ABI itself must change.

Current:

```text
GpuParticle (48 B)
├─ color
├─ position
├─ size
├─ rotation
├─ normalized age
├─ packed emitter/alive
└─ particle index
```

Future:

```text
Analytic simulation
        │
        ▼
   GpuParticle
        │
        ▼
     renderer
```

and:

```text
StatefulSimulationParticle / buffers
├─ position
├─ velocity
├─ age
├─ collision state
└─ custom persistent attrs
        │
        ▼
   GpuParticle
        │
        ▼
     renderer
```

and:

```text
FluidGridState
├─ velocity
├─ pressure
├─ density
└─ temperature
        │
        ├── volume renderer
        └── optional particle presentation
```

Renderer-facing storage and simulation storage must remain distinct.

---

# 24. Stateful storage layout

Do not immediately commit to one giant fixed AoS particle struct.

Aestra already performs attribute liveness, so the long-term design should allow storage specialization.

Conceptually:

```rust
pub struct SimulationStateLayout {
    pub persistent: Vec<ParticleAttribute>,
    pub transient: Vec<ParticleAttribute>,
}
```

Possible implementation options:

### Fixed AoS prototype

Good for the first implementation because it is simple and easy to validate.

```text
struct StateParticle {
    position
    velocity
    age
    lifetime
}
```

### Specialized AoS

Generate different packed structs depending on required attributes.

### SoA

Separate buffers:

```text
positions[]
velocities[]
ages[]
...
```

Potentially better for passes that only touch some attributes.

Do not choose the final layout by intuition.

Implement the first stateful prototype simply, then benchmark representative workloads before deciding whether specialized AoS or SoA is better.

---

# 25. Analytic optimization opportunities

The analytic backend should remain actively optimized even after stateful support exists.

Current likely pressure points include:

- dispatching over full capacity;
- per-slot emitter lookup;
- dead-slot rejection;
- repeated spawn reconstruction;
- repeated deterministic hashes;
- branch divergence;
- loop/cycle reconstruction;
- curve-driven spawn inversion.

Potential strategies to benchmark:

## Active worklist / active range generation

A small pass could derive currently relevant particle ranges or a compact worklist, then dispatch analytic evaluation only for useful logical particles.

Potential shape:

```text
emitter metadata
    ↓
active-range/work-count pass
    ↓
indirect compute dispatch
    ↓
analytic particle reconstruction
```

This could substantially improve sparse occupancy.

However, it adds:

- another pass;
- indirect dispatch complexity;
- worklist memory;
- synchronization.

Benchmark it against the current full-capacity path.

---

## Per-emitter dispatch

Avoid global slot → emitter binary search by dispatching an emitter's own logical range.

Potential downside:

- many emitters increase dispatch count.

Again, benchmark before selecting.

---

## Immutable spawn cache

Some spawn values can theoretically be cached:

```text
spawn time
initial origin
initial direction
speed
lifetime
random parameters
```

But do **not** make this universal.

Caching:

- consumes persistent memory;
- complicates continuous loops/cycle seeds;
- can erase some of the memory advantage of analytic execution.

Treat it as an optional optimization selected only when measurements justify it.

---

## Specialize simple analytic emitters

The compiler can eventually generate cheaper kernels for common subsets:

```text
ballistic only
ballistic + gravity
ballistic + drag
no turbulence
constant color
constant size
```

Aestra's semantic compiler makes this possible.

This is likely preferable to making all simple particles stateful merely to avoid repeated generic calculations.

---

# 26. Stateful runtime performance model

Stateful execution has a different cost profile.

Typical design:

```text
spawn pass
    ↓
persistent live state
    ↓
update pass
    ↓
alive/free management
    ↓
presentation
    ↓
indirect render
```

Advantages:

- initialize random values once;
- update only live particles;
- natural collision;
- natural accumulation;
- natural history-dependent behavior.

Costs:

- persistent VRAM;
- state read/write bandwidth;
- live/free-list maintenance;
- checkpoint storage;
- replay cost after backwards seek;
- simulation debt after culling.

Neither stateful nor analytic is universally superior.

---

# 27. Staged runtime model

Do not bolt fluid passes directly onto the particle update shader.

Introduce a generic compiled staged plan.

> **This is the same IR as the extensible-stages plan's portable Execution IR** (`ExecutionBlock` /
> `ComputeOp` / `ResourceAccess`, its §13). Design it once — `SimulationPassPlan` and `ExecutionBlock`
> are not two systems. Type correspondence in that plan's §44.1.

Conceptually:

```rust
pub struct SimulationPassPlan {
    pub id: SimulationPassId,
    pub reads: Vec<SimulationResource>,
    pub writes: Vec<SimulationResource>,
    pub dispatch: DispatchPlan,
    pub iterations: IterationPlan,
}
```

Resources might include:

```text
ParticleState
ParticleScratch
Grid2D
Grid3D
IndirectArgs
NeighborIndex
TransientBuffer
```

The backend can then schedule:

```text
pass 0
pass 1
pass 2 × N
pass 3
```

with the required WGPU pass ordering and ping-pong resources.

This infrastructure should be generic enough for:

- fluids;
- boids;
- PBD;
- neighbor queries;
- reaction-diffusion;
- future cloth-like VFX.

---

# 28. Authoring UX

Do not expose implementation complexity unnecessarily.

Default artist experience:

```text
Add Gravity
Add Collision
Add Fluid Solver
```

not:

```text
Choose Analytic / Stateful / Staged manually
```

The compiler should determine it.

The editor can show derived information in an advanced/profiler view:

```text
Simulation
  Class: Stateful
  Reason: Scene Collision requires previous particle state
  Seek: GPU checkpoint + replay
  Persistent state: 32 B / particle
  Capacity: 50,000
  Estimated state: 1.53 MiB
```

For a mixed effect:

```text
Simulation islands
  Sparks      Analytic
  Debris      Stateful
  Smoke       Staged
```

This is useful for debugging and optimization without making the user configure it manually.

---

# 29. Artifact format implications

`aestra-artifact` is currently format version `2` and persists one effect-wide `seek_mode`.

Introducing compiled simulation islands and their state layouts changes the compiled artifact contract.

Plan a new artifact version.

> **Coordinate this bump with the extensible-stages plan.** That plan also changes the compiled
> artifact (generic stages + execution IR). The compiled artifact bumps **once** (v2 → v3),
> encoding both simulation islands/state layouts *and* generic stages/execution IR — not two separate
> bumps. See `AESTRA_EXTENSIBLE_STAGES_PLUGINS_PROPERTIES.md` §44.5. The simulation `SimulationPassPlan`
> here is the same IR as that plan's `ExecutionBlock`/`ComputeOp` (its §44.1).

Recommended future compiled artifact data:

```text
Effect
├─ semantic temporal requirement summary
├─ simulation islands
│   ├─ execution class
│   ├─ temporal semantics
│   ├─ dependencies
│   ├─ persistent layout
│   └─ execution plan
├─ emitters
├─ renderers
└─ existing metadata
```

Keep backward compatibility policy explicit.

The current decoder rejects unsupported versions rather than silently guessing, which is good.

Do not serialize raw Rust/WGPU state.

Continue using explicit engine-neutral DTOs.

---

# 30. Relationship to the existing performance roadmap

The existing runtime performance plan already proposes evaluating a persistent incremental gameplay backend if the analytic kernel does not meet performance goals.

That remains correct, but this architecture changes one important conclusion:

> Stateful execution is needed **regardless of whether analytic performance is excellent**, because collision, constraints, and fluids require history.

Benchmarking should instead decide:

```text
For semantics that CAN use either implementation,
which backend is cheaper?
```

It should not decide whether Aestra supports stateful simulation at all.

Therefore:

```text
semantic necessity
    decides whether stateful/staged support must exist

benchmark data
    decides whether eligible simple effects should execute analytically
    or incrementally on a given backend/profile
```

---

# 31. Proposed runtime decision model

Eventually Aestra can distinguish:

```text
Required class
    = semantic minimum

Selected backend
    = performance/capability decision
```

For example:

```text
simple ballistic sparks

semantic minimum:
    Analytic

available implementations:
    Analytic
    Stateful

desktop discrete GPU:
    benchmark/policy may choose Analytic

mobile GPU:
    benchmark/policy may choose Stateful

editor exact seek:
    prefer Analytic
```

But do not implement automatic performance-based dual lowering initially.

First make semantic selection correct and deterministic.

Only add backend-specific cost-based selection when benchmarks justify it.

---

# 32. Recommended crate responsibilities

## `aestra-core`

Keep ownership of:

- authoring semantics;
- `SimulationDomain`;
- module/stage identities;
- collision/fluid authoring data when introduced.

Do not make `SimulationClass` an artist-authored property by default.

Potential additions later:

- portable collision primitives;
- grid-domain descriptors;
- solver authoring parameters.

---

## `aestra-compiler`

Own:

- module simulation requirements;
- requirement propagation;
- simulation island construction;
- persistent/transient attribute liveness;
- staged pass planning;
- diagnostics explaining class promotion;
- effect-level aggregate requirement.

This is the most important crate for the new architecture.

---

## `aestra-runtime`

Own:

- engine-neutral compiled simulation plans;
- canonical fixed-step simulation frames;
- temporal semantics;
- seek coordination;
- checkpoint policy and planning;
- CPU reference stateful execution;
- coherent mixed-island evaluation contract.

Keep Bevy/WGPU out.

---

## `aestra-artifact`

Persist:

- new compiled simulation island DTOs;
- temporal requirements;
- state layout descriptors;
- pass plans.

Version the change explicitly.

---

## `aestra-gpu`

Own:

- GPU lowering for analytic/stateful/staged plans;
- state buffer layouts;
- WESL compute programs;
- analytic direct-evaluation kernel;
- stateful spawn/update kernels;
- staged simulation shader plans;
- GPU-facing resource descriptors.

Keep `GpuParticle` as presentation data.

---

## `aestra-bevy-render`

Own:

- actual WGPU buffer allocation;
- compute dispatch;
- GPU checkpoint buffer copies;
- pipeline creation/cache;
- backend capability reporting;
- Bevy scene collision resource adaptation;
- presentation interpolation;
- render-world synchronization.

Generalize the existing trail GPU-checkpoint mechanism.

---

## `aestra-editor`

Own:

- preview vs exact seek UX;
- simulation-island diagnostics;
- checkpoint/replay progress for expensive seeks;
- profiler presentation;
- optional simulation quality controls.

---

## `aestra-viewer`

Remain the deterministic validation/capture surface.

Add mixed-mode and stateful conformance captures.

---

## `aestra-bench`

Extend the existing benchmark matrix rather than creating a new benchmark system.

Measure:

- analytic vs stateful;
- dense vs sparse;
- checkpoint restore;
- replay;
- mixed effects;
- collision;
- staged pass overhead;
- VRAM per execution class.

---

# 33. Implementation milestones

The milestones below are ordered to minimize architectural churn.

---

# Milestones 0–1 — shared foundation (see `aestra_shared_foundation_milestones.md`)

> This roadmap's original M0 (freeze baseline) and M1 (formalize simulation semantics) are **merged
> with the extensible-stages plan's M0/M1 into one shared front end** —
> `aestra_shared_foundation_milestones.md`, milestones **S0** and **S1**. Run those once; do not run
> this roadmap's M0/M1 separately.
>
> **S0** freezes a combined baseline: this roadmap's `aestra-bench` performance baselines (`B002`–
> `B005`, many-emitter / looping / curve-driven scenarios, CPU+GPU metrics) *and* the extensible
> plan's correctness fixtures, with GPU/CPU conformance preserved. Nothing changes — not particle
> semantics, not the 48-byte GPU ABI, not execution class.
>
> **S1** introduces `SimulationClass` / `TemporalSemantics` (derived, never authored) and keeps
> `SimulationSeekMode` as the resolved strategy only (the hardcoded `StatelessDirect` is **not** yet
> removed — that belongs to M2 below). Crucially, this roadmap's simulation module requirements go
> onto `ModuleMetadata` **in the same single extension** as the extensible plan's capabilities /
> reads / writes / multiplicity (that plan's §44.3) — one requirement system, co-designed, not two.
> The `SimulationDomain ≠ SimulationClass ≠ StageKind ≠ SimulationSeekMode` distinctions are
> documented and tested there.
>
> S1 acceptance already covers this roadmap's M1 bar: existing effects compile identically, all
> CPU/GPU conformance tests pass, and the compiler can report why each emitter is Analytic (and which
> requirement would promote it). Everything below (M2 onward) begins **after** S1.

---

# Milestone 2 — Derive simulation islands in the compiler

**Goal:** Replace the global assumption "the complete effect is stateless" with compiler-derived island semantics.

> **Status — first increment landed (unified U2).** The compiler now:
> - classifies each authored emitter by its derived `SimulationClass` from module requirements
>   (`EffectCompiler::classify_simulation`), naming the module that promoted it above `Analytic`;
> - **derives `CompiledEffect.seek_mode` from the aggregate class, removing the hardcoded
>   `StatelessDirect`** (§5.2) — analytic effects stay `StatelessDirect`, history-dependent effects
>   resolve to `RestartReplay` (checkpoint vs restart is a backend-capability choice, §5.3, made once
>   a checkpoint backend exists). All S0 baselines stay green: existing effects are unchanged.
>
> **Update — U3 first increment landed.** `CompiledEmitter.simulation_class` now *persists* the
> derived class (populated by the compiler, one class per emitter-region — the initial island
> granularity of §13), and the compiled-artifact format was bumped **v2 → v3** to carry it
> (`SimulationClassV1`), the single coordinated compiled bump (§44.5). The S0 artifact baseline moved
> with it (it tracks `CURRENT_ARTIFACT_VERSION`), and a test proves a non-`Analytic` class round-trips.
>
> **Still to do in M2/M3/M4:** a first-class `CompiledSimulationIsland` grouping with dependency edges
> (islands that span emitters), the persistent/transient **state-layout** split (M4), and the
> simulation-state vs presentation-state ABI separation. Those build on the per-emitter class landed
> here.

### Initial implementation

Use each compiled emitter-region as the initial island seed.

For each island:

1. collect module requirements;
2. determine temporal dependency;
3. determine synchronization requirement;
4. derive execution class;
5. record dependency edges.

### Promotion

Implement deterministic promotion:

```text
Direct
    + PreviousState
    → Stateful

Stateful
    + iterative/neighborhood/grid requirement
    → Staged
```

### Effect summary

Derive an aggregate effect requirement for compatibility/debugging, but do not use the aggregate to unnecessarily replay analytic islands.

### Important current-code change

Remove the unconditional compiler assumption:

```rust
seek_mode: SimulationSeekMode::StatelessDirect
```

once the new representation is ready.

### Acceptance criteria

- Existing effects produce only analytic islands.
- A test-only stateful module causes only its island to become stateful.
- Mixed analytic/stateful compilation is represented correctly even before the GPU backend executes it.
- Compiler diagnostics identify the module that caused promotion.

---

# Milestone 3 — Update compiled artifact format

**Goal:** Make hybrid simulation a stable portable compiled contract.

### Add to artifact DTO

- simulation islands;
- execution class;
- temporal semantics;
- dependencies;
- persistent state layout;
- staged plan descriptors when present.

### Versioning

Bump the compiled artifact format from the current version 2.

Keep the existing explicit-version rejection behavior.

### Tests

- analytic artifact roundtrip;
- stateful artifact roundtrip;
- mixed effect roundtrip;
- invalid state-layout rejection;
- invalid island dependency rejection;
- deterministic serialization where expected.

### Acceptance criteria

A compiled effect can be serialized/reloaded without losing hybrid simulation semantics.

---

# Milestone 4 — Separate simulation-state and presentation-state ABIs

**Goal:** Prepare GPU/CPU runtime storage without changing visible rendering.

> **Status — engine-neutral descriptor landed (unified U3/M4).** `SimulationStateLayout
> { persistent, transient }` exists in `aestra-runtime`, distinct from the 48-byte `GpuParticle`
> presentation ABI (untouched). It is *derived* from an emitter's class
> (`CompiledEmitter::simulation_state_layout` → `SimulationStateLayout::for_class`): analytic emitters
> get an empty layout (`requires_state_buffer() == false`, so analytic effects allocate no state
> buffer); stateful/staged emitters get the fixed prototype persistent set (position/velocity/age/
> lifetime). Not persisted — trivially derivable for now; a specialized per-module layout can be
> persisted (with a compiled bump) once it stops being derivable.
>
> **Still to do:** the actual GPU state-buffer allocation + stateful update kernel — that is the
> stateful backend (M5 CPU reference, M6 GPU), which consumes this descriptor. SoA vs specialized-AoS
> is deferred to benchmarking (§24).

### Keep

```text
GpuParticle = 48-byte presentation record
```

### Add engine-neutral state descriptors

Conceptually:

```rust
SimulationStateLayout {
    persistent
    transient
}
```

### GPU prototype

Create a separate state buffer for test-only stateful islands.

Start with a simple fixed prototype layout:

```text
position
velocity
age
lifetime
```

Do not attempt final SoA specialization yet.

### Acceptance criteria

- Renderer still consumes the existing presentation particle path.
- Analytic effects allocate no unnecessary stateful buffer.
- Stateful test island gets a separate simulation state buffer.
- Presentation ABI conformance tests remain valid.

---

# Milestone 5 — Minimal stateful CPU reference backend

**Goal:** Establish deterministic stateful semantics before relying on GPU-only behavior.

> **Status — landed (unified M5, first increment).** `aestra-runtime`'s `stateful` module provides a
> minimal deterministic stateful reference: `StatefulSimulation` maintains persistent per-particle
> state (position/velocity/age/lifetime), spawns deterministically from `(seed, ordinal)` via
> splitmix64, advances on the canonical 60 Hz fixed tick with semi-implicit Euler integration, retires
> dead particles, and extracts renderer-neutral `ParticleSample`s. A `Clone` is a checkpoint; backward
> seeks panic (restore + replay forward, §16). All M5 acceptance criteria are covered by tests:
> deterministic from seed, `advance_to_tick` == tick-by-tick replay, checkpoint-clone-then-replay ==
> uninterrupted forward run, presentation reflects integrated motion, and capacity bounds the live
> count. It is a standalone reference (not yet wired through the compiler) — deliberately, per the
> milestone's "backend validation, not feature value".
>
> **Still to do:** wire the stateful reference to compiled stateful emitters (so `evaluate` routes a
> stateful island through it), and the GPU stateful backend (M6). Both consume the M4 state layout.

### Implement

A minimal stateful particle lifecycle:

```text
spawn
initialize persistent state
fixed-tick update
death
presentation extraction
```

Use the existing 60 Hz playback clock.

A test module can perform:

```text
velocity integration
gravity
```

even though gravity already has an analytic solution.

The purpose is backend validation, not feature value.

### Why use a semantically duplicated test?

Because the same simple motion can be compared between:

```text
analytic implementation
stateful implementation
```

without collision complexity.

### Acceptance criteria

- deterministic replay from seed;
- restart/replay reaches identical state;
- checkpoint clone + replay reaches identical state;
- same fixed-tick state across repeated runs;
- presentation extraction matches expected positions.

---

# Milestone 6 — Minimal stateful GPU backend

**Goal:** Prove persistent GPU particle simulation end-to-end.

> **Status — first increment landed (unified M6).** A GPU compute kernel integrates a persistent
> per-particle state buffer with the same semi-implicit Euler step as the M5 CPU reference, dispatched
> once per fixed tick within a single pass (WebGPU orders dispatches, so state accumulates across
> ticks with no readback during simulation). `bevy/aestra-bevy-render/tests/stateful_conformance.rs`
> proves it on real GPU compute: GPU state matches the CPU reference at canonical tick counts
> (1/5/60/240), and state persists and accumulates incrementally (age accrues exactly one dt per
> tick). Verified on a real adapter; skips on GPU-less CI, and is wired into the `gpu-visual`
> workflow so it runs with `AESTRA_REQUIRE_GPU_CONFORMANCE=1`.
>
> **Real-backend integration — started in `src`.** Beyond the conformance proofs, the production path
> has begun: `aestra-gpu`'s artifact now carries a `GpuSimulationState { stride, records }` descriptor
> on `GpuEffectDynamics`, computed from the enabled stateful emitters' capacity and the M4 state
> layout — the engine-neutral sizing the render backend allocates its persistent state buffer from.
> Empty for analytic effects (every current effect), so nothing changes for them.
>
> **Spawn RNG — the u64 blocker is solved (in `src`).** GPU spawn needs the same deterministic
> splitmix64 the CPU reference uses, but WGSL has no native `u64`. `aestra_gpu::STATEFUL_SPAWN_RNG_WGSL`
> now emulates `u64` with `u32` pairs (`vec2<u32>`) — full 64-bit add/mul/shr and splitmix64 — and
> `stateful_conformance.rs` proves the GPU `spawn_launch_direction` matches
> `aestra_runtime::StatefulSimulation::launch_direction` across many seeds/ordinals within 1e-6 (a
> tight bound: any error in the u64 math produces a wildly different hash). The runtime exposes
> `StatefulSimulation::{splitmix64, launch_direction}` as the canonical definitions both sides conform
> to. This is production shader code in `aestra-gpu`, reusable by the eventual spawn pipeline.
>
> **Spawn kernel + free-list allocator — landed and proven.** The GPU spawn+integrate loop reproduces
> the M5 CPU reference over a no-death window (using the u64 RNG), and `aestra_gpu::STATEFUL_FREE_LIST_WGSL`
> is a GPU atomic free-list allocator proven to hand out distinct slots under parallel allocation — the
> hard property behind dead-slot recycling. Both are production WGSL in `aestra-gpu`.
>
> **Still to do — the rest of the harder half:** assemble integrate + spawn + death + the free list
> into one per-tick loop with per-particle identity (particles matched by their deterministic spawn
> ordinal, so parallel slot assignment need not match the CPU), presentation extraction (state →
> 48-byte `GpuParticle`), and finally wiring allocate + dispatch into `aestra-bevy-render/src/gpu.rs`
> — which then runs a real stateful effect end-to-end, meaningful precisely because the proven kernels
> exist.

### GPU passes

Implement:

```text
spawn/update persistent state
        ↓
presentation extraction
        ↓
existing alive/indirect/render path
```

Reuse existing alive/dead infrastructure where useful, but do not force the analytic dead-slot model onto the stateful implementation if a live/free-list design is better.

### CPU/GPU conformance

Compare stateful CPU reference and native GPU output at canonical frames.

### Acceptance criteria

- stateful particles advance incrementally on GPU;
- GPU state persists between ticks;
- no CPU readback is required for ordinary simulation;
- existing analytic backend is unaffected;
- mixed analytic + stateful effects render correctly.

---

# Milestone 7 — Generic GPU checkpoint backend

**Goal:** Generalize the existing trail checkpoint approach to simulation state.

> **Status — core seek model proven on GPU (unified M7, first increment).**
> `stateful_conformance.rs` now proves the backward-seek contract on real GPU compute: a snapshot of
> the persistent state buffer (GPU→GPU copy), an overshoot forward, then a restore (GPU→GPU copy) and
> forward replay reaches the *same* state as an uninterrupted forward run to the seek target — with no
> CPU readback except the final check (§4.4/§19), and the simulation never run with negative `dt`
> (§16). Verified on a real adapter and wired into the `gpu-visual` CI.
>
> **Still to do:** a generic runtime-owned checkpoint *store* (cadence, byte budget, nearest-checkpoint
> lookup, invalidation) around this GPU-resident snapshot — i.e. generalizing `CheckpointStore` /
> `CheckpointContext` to simulation state and driving snapshot/restore from the seek planner, rather
> than the hand-scheduled snapshot in the test. This increment proves the GPU mechanism; the policy
> layer is the follow-up.

### Reuse trail design principles

- GPU-resident snapshots;
- GPU-to-GPU buffer copies;
- bounded memory;
- recycled checkpoint allocations;
- no normal readback.

### Generalize runtime context

Add sufficient checkpoint identity for:

- effect revision;
- instance;
- seed;
- backend;
- simulation state layout;
- history-relevant parameter/context fingerprint.

### Seek

Use existing planning semantics:

```text
backward target
    ↓
nearest checkpoint
    ↓
GPU restore
    ↓
fixed-step replay
```

### Acceptance criteria

- seeking a stateful particle effect backward never displays a checkpoint as the final state;
- final exact state equals uninterrupted forward simulation;
- GPU checkpoints do not require CPU round trips;
- stale checkpoints are rejected after relevant parameter changes.

---

# Milestone 8 — Mixed-island coherent seeking

**Goal:** Make one effect safely combine analytic, stateful, and eventually staged islands.

### Add an effect-level seek coordinator

Given:

```text
target presentation time
```

coordinate:

```text
analytic direct evaluation
stateful restore/replay
child effect mapping
```

and publish only a coherent exact presentation.

### Nested effects

Extend checkpoint identity/scheduling so multiple active child instances cannot accidentally reuse incompatible state.

### Presentation interpolation

Add previous/current state presentation interpolation for stateful particles so high-refresh rendering does not visually step at the 60 Hz simulation clock.

### Acceptance criteria

- analytic + stateful effect seeks backward correctly;
- both appear at the same requested logical time;
- stateful particles do not visibly jump between checkpoint times;
- 120/144 Hz presentation is smooth while simulation remains fixed-step;
- pause/restart/loop/continuous-loop tests cover mixed islands.

---

# Milestone 9 — Hybrid performance benchmark and analytic-kernel optimization

**Goal:** Make performance policy evidence-driven.

### Extend `aestra-bench`

Add identical semantic workloads implemented as:

```text
analytic
stateful
```

where both are valid.

Test:

```text
1k
10k
100k
500k
```

at:

```text
1%
5%
25%
50%
100%
```

occupancy.

Also test:

- short vs long lifetime;
- simple vs curve-driven emission;
- one vs many emitters;
- one vs many effect instances.

### Measure

```text
GPU simulation time
bytes / slot
bytes / alive particle
checkpoint memory
restore cost
replay cost
dispatch count
workgroups
CPU preparation
```

### Analytic experiments

Measure, not assume:

- current full-capacity dispatch;
- compact active worklist;
- per-emitter dispatch;
- specialized simple kernels;
- optional spawn caching.

### Output

Define a documented crossover matrix.

Do not yet make the runtime automatically switch based on a single GPU's result.

### Acceptance criteria

Aestra can answer:

> For which workloads is analytic actually cheaper than stateful on the tested hardware?

and:

> How much does sparse capacity hurt the current analytic path?

---

# Milestone 10 — Stateful collision primitives

**Goal:** Add the first real feature that semantically requires history.

### Start portable

Implement engine-independent collision with simple authored colliders:

- plane;
- sphere;
- AABB.

Support:

- kill;
- bounce;
- restitution;
- friction;
- optional sliding.

### Events

Connect collision results to the existing `OnCollision` event semantics.

### Compiler

Collision modules declare:

```text
temporal requirement = PreviousState
```

and automatically promote their island.

### Acceptance criteria

- no manual "stateful" toggle required;
- collision causes compiler promotion;
- CPU/GPU fixed-tick collision conformance;
- backward seek via checkpoint/replay reproduces uninterrupted forward simulation;
- `OnCollision` event ordering is deterministic.

---

# Milestone 11 — Collision input provider abstraction

**Goal:** Prepare for engine scene collision without coupling portable crates to Bevy.

### Define semantic/backend boundary

Portable plan describes what collision capability is required.

The Bevy adapter provides actual scene data/resources.

Potential sources later:

- explicit Aestra colliders;
- SDF;
- depth buffer;
- engine physics queries;
- mesh/acceleration structure depending on backend capability.

### Historical input rule

Define whether a collision provider is:

```text
time-addressable
recordable
checkpointed
forward-only
```

### Acceptance criteria

- portable crates contain no Bevy physics dependency;
- backend capability failure is explicit;
- exact backward seeking is only advertised when historical collision inputs can be reconstructed.

---

# Milestone 12 — Preview vs exact seek modes

**Goal:** Keep the editor responsive when stateful effects become expensive.

### Add

```text
SeekQuality::Preview
SeekQuality::Exact
```

### Preview policy

Allow bounded approximation:

- coarser stateful timestep;
- reduced collision complexity;
- fewer particles;
- reduced staged solver iterations.

### UX

When the user releases the timeline cursor:

```text
request exact
    ↓
restore/replay
    ↓
replace preview with authoritative result
```

### Acceptance criteria

- rapid scrubbing remains responsive;
- preview never claims to be authoritative;
- expensive reconstruction is bounded per editor frame;
- exact result is deterministic.

---

# Milestone 13 — Generic staged simulation plan

**Goal:** Build multi-pass infrastructure before implementing real fluids.

### Runtime/compiler

Add generic:

- simulation resources;
- ordered pass graph;
- read/write dependencies;
- iterations;
- ping-pong resources;
- dispatch shape;
- transient scratch lifetime.

### GPU

Generate and execute ordered WESL/WGSL compute passes.

### First validation workload

Do **not** start with a full 3D fluid.

Use a small deterministic test such as:

- 2D diffusion;
- simple grid advection;
- reaction-diffusion.

### Acceptance criteria

- multiple ordered passes execute deterministically;
- barriers/resource transitions are correct;
- ping-pong state survives checkpoints;
- staged state can restore/replay;
- pass-level GPU timestamps are available.

---

# Milestone 14 — First fluid prototype

> **Superseded — fluids ship as a GPU-only plugin.** Per the reconciliation with
> `AESTRA_EXTENSIBLE_STAGES_PLUGINS_PROPERTIES.md` (its §44.6), Aestra has **no first-party
> CPU-reference fluid**. The generic staged infrastructure (M13) stays first-party, but the fluid
> solver itself is an external GPU-only plugin (that plan's M13), declaring `cpu_reference =
> Unavailable`. Treat this milestone as the *staged-infrastructure* proof, not a first-party fluid
> feature: the acceptance criteria below that assume a CPU-reproducible fluid are replaced by
> GPU-vs-GPU determinism (same asset + seed → same frames). Do not add a first-party fluid solver or
> CPU fluid fixtures.

**Goal:** Prove real stateful grid simulation using generic staged infrastructure.

### Suggested order

```text
2D grid smoke
    ↓
3D grid smoke
```

Minimum stages:

```text
inject
advect velocity
advect density
forces
divergence
pressure iterations
projection
```

### Rendering

Keep simulation resource design independent from how it is presented.

Possible first presentation:

- particles sampling the grid;
- simple slice/debug display;
- later volume renderer.

Do not require the particle presentation ABI to store fluid state.

### Acceptance criteria

- deterministic fixed-step fluid simulation;
- bounded checkpoint memory;
- backward seek via restore/replay;
- analytic emitters can inject deterministic input into the fluid;
- fluid can drive stateful particles.

---

# Milestone 15 — Advanced dependency graph

**Goal:** Support complex cross-system effects safely.

Examples:

```text
analytic sparks
    ↓ inject heat
fluid

fluid velocity
    ↓
stateful embers

stateful collision
    ↓ OnCollision
child burst
```

### Compiler

Expand island dependency analysis.

Promote downstream work when necessary.

Detect cycles and determine whether they require one staged island.

### Acceptance criteria

- dependency ordering is explicit;
- impossible direct-seek assumptions are rejected;
- stateful → analytic feedback cannot silently remain analytic;
- cycles have deterministic staged semantics or clear diagnostics.

---

# Milestone 16 — Production optimization and policy

**Goal:** Turn the hybrid system into an AAA-oriented runtime rather than merely a feature-complete one.

### Benchmark

- hundreds/thousands of effect instances;
- mixed analytic/stateful/staged effects;
- checkpoint pressure;
- collision-heavy scenes;
- fluid grids;
- culling/reappearance;
- nested effects;
- low occupancy;
- high occupancy.

### Investigate only if measured

- global stateful particle arena;
- cross-effect batching;
- indirect multi-dispatch;
- shared free lists;
- state-layout specialization;
- SoA conversion;
- backend-specific execution-class preference;
- adaptive checkpoint cadence.

### Editor profiler

Expose:

```text
island execution class
capacity
alive
occupancy
persistent VRAM
checkpoint VRAM
GPU simulation cost
replay cost
dispatch count
solver pass count
```

### Acceptance criteria

- hybrid performance regressions are tracked in CI/baselines;
- memory is bounded;
- expensive effects are diagnosable in the editor;
- execution policy is supported by benchmark data.

---

# 34. Milestone dependency summary

Recommended order:

```text
S0   shared baseline        (aestra_shared_foundation_milestones.md)
 │
 ▼
S1   shared semantics + identity + one ModuleMetadata extension
 │
 ▼
M2   compiler islands
 │
 ▼
M3   artifact format        (single coordinated compiled v2→v3 bump, §29)
 │
 ▼
M4   simulation/presentation storage split
 │
 ├───────────────┐
 ▼               │
M5 CPU stateful  │
 │               │
 ▼               │
M6 GPU stateful  │
 │               │
 ▼               │
M7 checkpoints   │
 │               │
 ▼               │
M8 mixed seek    │
 │               │
 ▼               │
M9 benchmarks/analytic optimization
 │
 ▼
M10 collision primitives
 │
 ▼
M11 scene collision abstraction
 │
 ▼
M12 preview/exact editor seeking
 │
 ▼
M13 generic staged simulation
 │
 ▼
M14 fluids
 │
 ▼
M15 advanced dependency graph
 │
 ▼
M16 production optimization
```

Some benchmark work should of course continue throughout all milestones.

---

# 35. What not to do

## Do not convert all existing particles to stateful execution

That would throw away:

- random-access seeking;
- zero simulation debt after culling;
- cheap arbitrary-time preview;
- low checkpoint memory;
- easy parameter iteration.

---

## Do not keep one effect-wide simulation class as the final design

A mixed effect should not force hundreds of thousands of analytic sparks into checkpoint/replay merely because 500 debris particles collide.

---

## Do not put physics state into `GpuParticle`

Keep presentation and simulation storage independent.

---

## Do not simulate backwards with negative `dt`

Restore and replay forward.

---

## Do not implement normal GPU checkpoints through CPU readback

Generalize the existing GPU-resident trail snapshot mechanism.

---

## Do not make `SimulationDomain` mean analytic/stateful

It describes simulation topology, not temporal execution semantics.

---

## Do not make artists manually manage execution classes

Modules declare semantic requirements; the compiler derives classes.

---

## Do not implement fluids before generic staged execution exists

Otherwise the first fluid solver will accidentally become a second ad-hoc runtime architecture.

---

## Do not make checkpoint validity depend only on effect + seed once scene interaction exists

External historical inputs and instance identity matter.

---

## Do not optimize the analytic kernel blindly

Use the benchmark infrastructure that Aestra already has.

---

# 36. Suggested final architecture

```text
                        EffectAsset
                            │
                            ▼
                       aestra-core
                            │
                            ▼
                     aestra-compiler
                            │
             semantic requirement analysis
                            │
                 simulation dependency graph
                            │
        ┌───────────────────┼────────────────────┐
        ▼                   ▼                    ▼
    Analytic island     Stateful island       Staged island
        │                   │                    │
 direct time eval      persistent layout     pass/resource plan
        │                   │                    │
        └───────────────────┼────────────────────┘
                            │
                            ▼
                    CompiledEffect
                    / aestra-artifact
                            │
                            ▼
                      EffectInstance
                            │
                 shared PlaybackClock
                            │
           ┌────────────────┼────────────────┐
           ▼                ▼                ▼
      direct evaluate   fixed-step state   staged state
           │                │                │
           │          checkpoint/replay      │
           │                │          checkpoint/replay
           └────────────────┼────────────────┘
                            ▼
                   PresentationSnapshot(t)
                            │
                            ▼
                 48-byte GpuParticle ABI
                 + other renderer resources
                            │
                            ▼
                         Renderer
```

---

# 37. Final recommendation

The current codebase is already well positioned for this change.

The most important existing pieces are:

1. `aestra-runtime` already has fixed-step playback and generic seek/checkpoint planning.
2. `aestra-bevy-render` already proves GPU-resident checkpoint/replay with trails.
3. `aestra-compiler` already owns module metadata, stages, attribute flow, and liveness.
4. `aestra-gpu` already keeps presentation data separate from trail auxiliary state.
5. `aestra-bench` already contains the right sparse-capacity benchmark philosophy.

The central architectural change is therefore:

> **Move from an effect-wide assumption of direct analytic evaluation to compiler-derived simulation islands with explicit temporal requirements.**

Then build stateful particles and staged simulation behind the existing runtime/backend boundaries.

Aestra's long-term identity should be:

> **A hybrid VFX simulation system with random-access time wherever the effect semantics allow it.**

This is stronger than either:

```text
"everything is stateless"
```

or:

```text
"everything is stateful"
```

because it allows Aestra to retain its unusually strong timeline behavior while still growing toward:

- production collision;
- constraints;
- neighbor interactions;
- fluids;
- staged GPU solvers;
- large particle counts;
- deterministic capture;
- responsive authoring;
- AI-assisted effect iteration;
- portable engine backends.

The guiding rule should remain:

> **Use the least stateful execution model capable of preserving the effect's semantics, and let measurements—not assumptions—decide between equivalent implementations.**
