# Aestra Runtime Benchmarking & Profiling Plan

## Purpose

Aestra aims to become a production-grade VFX authoring and runtime system suitable for demanding real-time games.

For that goal, performance cannot be evaluated only with screenshots, average FPS, or isolated particle-count demos. Aestra needs a repeatable benchmark and profiling framework that can answer:

- How much CPU frame time does Aestra consume?
- How much GPU frame time does Aestra consume?
- How does cost scale with particle capacity?
- How does cost scale with *alive* particle count?
- How does cost scale with effect-instance count?
- How does cost scale with emitter count?
- How expensive are loops, curves, nesting, materials, and parameter updates?
- Where are CPU allocations occurring?
- Which costs come from simulation versus render integration?
- Can performance regressions be detected automatically?
- At what point does the current analytical GPU runtime stop scaling well enough for AAA usage?

The immediate objective is **measurement before large-scale optimization**.

---

# 1. Current Architecture Assessment

The current architecture is promising for performance work because major responsibilities are already separated:

- engine-independent runtime;
- CPU/reference execution;
- GPU lowering/runtime representation;
- Bevy/WGPU rendering integration;
- CPU/GPU conformance testing;
- native GPU CI infrastructure.

This separation makes it possible to benchmark:

1. runtime semantics;
2. CPU simulation;
3. GPU simulation;
4. render preparation;
5. GPU rendering;
6. whole-frame integration;

independently.

However, several parts of the current implementation are likely to become performance bottlenecks at production scale.

---

# 2. Critical Performance Risks

## 2.1 GPU artifacts appear to be reconstructed every frame

The current GPU update path reconstructs a `GpuEffectArtifact` from an effect instance during updates.

Conceptually:

```rust
GpuEffectArtifact::from_instance(&player.instance)
```

The artifact construction allocates and initializes particle storage according to total slot capacity.

This is problematic when the update path only needs dynamic emitter or renderer inputs.

For example, a 100,000-slot effect can result in CPU-side initialization of 100,000 particle structures even though persistent GPU particle storage already exists.

### Risk

Cost scales with particle **capacity**, even when:

- very few particles are alive;
- the compiled effect did not change;
- only time changed;
- only one dynamic parameter changed.

### Required direction

Separate immutable compiled data from mutable runtime state.

Target architecture:

```text
CompiledEffect
    |
    v
GpuEffectPrototype
    |- immutable emitter layout
    |- immutable renderer layout
    |- material variants
    |- pipeline requirements
    |- particle capacity
    |- buffer layouts
    |
    v
GpuEffectInstance
    |- time
    |- seed
    |- transform
    |- runtime parameters
    |- dirty flags
    |- persistent GPU buffers
```

Particle storage should normally be allocated once when the GPU instance is created or resized.

---

## 2.2 GPU simulation currently scales with total particle capacity

The current analytical GPU simulation dispatches across the entire slot range.

Conceptually:

```text
dispatch(total_slots)
```

Each slot reconstructs whether a particle should exist at the current absolute effect time.

This gives Aestra useful properties:

- deterministic seeking;
- deterministic replay;
- easy editor scrubbing;
- no accumulated simulation drift;
- strong CPU/GPU conformance;
- evaluation at arbitrary timestamps.

However, realtime runtime cost is approximately tied to:

```text
total particle capacity
```

rather than:

```text
alive particles + newly spawned particles
```

### Critical benchmark

Occupancy must become a first-class performance metric:

```text
occupancy = alive_particles / allocated_particle_slots
```

Benchmark at:

- 1%;
- 10%;
- 50%;
- 100%.

If a 100,000-capacity effect costs approximately the same with 1,000 alive particles and 100,000 alive particles, then sparse effects will be particularly expensive.

---

## 2.3 Per-slot emitter lookup

Each particle slot currently searches emitters to determine which emitter owns the slot.

If the search is linear, simulation complexity becomes influenced by both:

```text
particle slots x emitter count
```

### Required benchmark

Hold capacity constant:

```text
500k slots / 1 emitter
500k slots / 4 emitters
500k slots / 16 emitters
500k slots / 64 emitters
```

If GPU time grows significantly with emitter count, emitter ownership lookup needs redesign.

Potential solutions include:

- dispatch separately by emitter;
- precomputed slot-to-emitter mapping;
- packed emitter ranges;
- hierarchical lookup;
- GPU-side effect execution tables.

---

## 2.4 Continuous effects can increase analytical reconstruction cost

Continuous effects may need to search previous effect cycles when particle lifetime exceeds effect duration.

Example:

```text
effect duration     = 0.1 s
particle lifetime   = 10 s
```

Potentially around 100 historical cycles are relevant.

This is a legitimate artist-authored configuration and therefore must not create an uncontrolled runtime cost.

### Required benchmark axis

Define:

```text
loop_pressure = particle_lifetime / effect_duration
```

Benchmark:

- 1x;
- 4x;
- 16x;
- 64x;
- pathological stress cases.

---

## 2.5 Curve-driven emission is expensive in analytical mode

Spawn-time reconstruction for arbitrary emission curves can require iterative inverse evaluation.

If a binary search runs for every candidate slot every frame, this becomes expensive at high capacity.

### Benchmark independently

Compare:

- burst;
- constant rate;
- simple linear curve;
- 2-key curve;
- 4-key curve;
- 8-key curve;
- animated/runtime-modified curve.

Measure cost per 1,000 slots.

---

## 2.6 Global GPU atomics

Simulation currently performs atomic operations for alive/dead bookkeeping and indirect draw counts.

At high particle counts, global atomic contention may become important.

### Benchmark

Measure GPU simulation with:

- nearly all particles alive;
- nearly all particles dead;
- 50/50 distribution;
- one renderer;
- several renderers.

Investigate whether dead-slot bookkeeping is needed in the current analytical backend.

If the dead list is unused by subsequent stages, removing it may reduce:

- VRAM usage;
- buffer writes;
- atomic contention.

An incremental backend may need a free/dead list later, but the analytical backend does not necessarily need to maintain the same data structures.

---

## 2.7 One simulation submission per effect instance

The render path currently processes effects independently.

This is simple and robust, but production scenes often contain:

- hundreds of small impact effects;
- muzzle flashes;
- sparks;
- environmental effects;
- trails;
- destruction fragments;
- ambient effects.

The scaling test must therefore not be limited to one enormous effect.

### Critical benchmark

Keep total particle count approximately constant:

```text
1 effect    x 100,000 particles
10 effects  x 10,000 particles
100 effects x 1,000 particles
1,000 effects x 100 particles
```

Measure:

- CPU preparation time;
- number of compute passes;
- number of dispatches;
- bind group overhead;
- GPU submission overhead;
- draw calls;
- render extraction time.

If instance count becomes a major bottleneck, investigate:

- batched compute passes;
- global particle arenas;
- global emitter tables;
- multi-effect dispatch;
- indirect multi-draw strategies.

---

## 2.8 Bind groups and immutable GPU state

Bind groups appear to be prepared repeatedly as part of the render preparation path.

Immutable or rarely changing resources should not be recreated every frame.

### Target behavior

```text
compiled effect changed
    -> rebuild prototype

GPU buffers reallocated
    -> rebuild dependent bind groups

material binding changed
    -> rebuild material resources

time changed
    -> update globals only

transform changed
    -> update transform/global data only

runtime parameter changed
    -> update affected values only
```

This requires explicit dirty-state tracking.

---

## 2.9 Material preparation

Semantic material preparation should also be profiled separately.

Questions to answer:

- Are materials rebuilt every frame?
- Are uniform buffers rewritten when values are unchanged?
- Do shared materials actually share GPU resources?
- What is the cost of unique materials per effect?
- What is the cost of texture-heavy materials?
- How many pipeline/material variants are generated?

---

# 3. Analytical Runtime vs Incremental Runtime

Aestra's analytical execution model has real value and should not be discarded without measurement.

It is especially strong for:

- editor scrubbing;
- seeking;
- deterministic previews;
- replay;
- conformance;
- arbitrary-time evaluation;
- debugging;
- offline validation.

However, the AAA gameplay runtime may ultimately require a second execution strategy.

## Analytical runtime

Approximate cost:

```text
O(total_capacity x reconstruction_cost)
```

Advantages:

- deterministic;
- seekable;
- easy to validate;
- excellent editor semantics.

## Incremental realtime runtime

Typical structure:

```text
Spawn
    |
    v
allocate from free/dead list

Update
    |
    v
simulate alive particles

Cull / Compact
    |
    v
alive list

Render
```

Approximate cost:

```text
O(alive_particles + newly_spawned_particles)
```

Advantages:

- better for sparse particle populations;
- naturally suited to long-running gameplay;
- avoids reconstructing historical state every frame.

## Proposed long-term direction

Do not choose immediately.

Instead, benchmark the analytical backend and determine its practical scaling envelope.

Possible final architecture:

```text
                 Aestra Effect IR
                       |
              +--------+--------+
              |                 |
              v                 v
     Analytical Backend   Incremental Backend
              |                 |
      seek / preview /     realtime gameplay
      reference / tests
```

Both backends should use the same semantic effect representation wherever possible.

---

# 4. CPU Reference Runtime

The CPU implementation is valuable as:

- semantic reference;
- CPU/GPU conformance oracle;
- test backend;
- headless backend;
- fallback backend;
- debugging implementation.

It does not necessarily need to be the highest-performance shipping backend.

Still, it should be benchmarked.

## Areas to investigate

### Per-evaluation allocations

Avoid unnecessary temporary vectors and repeated heap allocations.

### Per-particle invariant work

Emitter transforms, quaternion normalization, static matrices, and other invariant data should be precomputed outside particle loops.

### Nested effects

Nested evaluation can introduce:

- temporary vectors;
- cloned instance paths;
- repeated transform composition;
- recursive allocation.

Benchmark both depth and breadth.

---

# 5. Profiling Architecture

Aestra should separate two related concepts.

## Effect telemetry

Answers:

> What is the effect doing?

Example values:

- alive particles;
- capacity;
- emitter count;
- renderer count;
- draw calls;
- dispatches;
- estimated memory;
- occupancy;
- collision count;
- screen coverage.

This can feed editor UX.

## Machine profiler

Answers:

> Where did CPU/GPU time actually go?

Example values:

- runtime evaluation time;
- GPU preparation time;
- buffer-upload time;
- simulation GPU time;
- render GPU time;
- allocations;
- bind-group preparation time.

These two systems should interoperate, but they should not be the same abstraction.

---

# 6. Tracy as the Primary Interactive Profiler

Use Bevy's tracing integration with Tracy for routine CPU analysis.

Recommended Bevy features:

```text
trace_tracy
trace_tracy_memory
```

Add explicit Aestra spans.

Suggested names:

```text
aestra::runtime::advance
aestra::runtime::evaluate_cpu

aestra::gpu::prepare_instance
aestra::gpu::artifact_update
aestra::gpu::material_prepare
aestra::gpu::buffer_upload
aestra::gpu::bind_groups

aestra::gpu::simulate
aestra::gpu::render
```

Tracy should make it possible to answer:

- what Aestra costs per frame;
- what scales with effect count;
- whether allocations occur every frame;
- whether main-thread stalls occur;
- where render-world preparation time is spent.

---

# 7. GPU Profiling

CPU traces alone are insufficient.

Add real GPU timestamp queries around:

```text
Aestra simulation reset
Aestra simulation
Aestra render
```

Where useful, break large GPU operations down further.

Record GPU timings in the benchmark output.

## Vendor tools for deeper analysis

Use when investigating GPU-specific bottlenecks:

- NVIDIA Nsight Graphics;
- AMD Radeon GPU Profiler;
- PIX on Windows/DX12;
- RenderDoc;
- Xcode Metal tools on macOS.

Tracy remains the everyday profiler.

Vendor tools are for understanding:

- occupancy;
- wave/warp utilization;
- atomic contention;
- memory bandwidth;
- cache behavior;
- branch divergence;
- shader instruction pressure.

---

# 8. Benchmark Application

Create a dedicated executable rather than relying only on tests.

Suggested layout:

```text
apps/
    aestra-bench/

benchmarks/
    scenarios/
        baseline.*
        dense.*
        sparse.*
        emitter_stress.*
        instance_stress.*
        continuous_stress.*
        curves_stress.*
        nesting_stress.*
        materials_stress.*
        overdraw_stress.*
```

The benchmark executable should:

1. load a deterministic scenario;
2. warm up pipelines and GPU resources;
3. run for a fixed capture interval;
4. collect CPU and GPU metrics;
5. calculate distributions;
6. write machine-readable JSON;
7. optionally export a Tracy capture;
8. print a human-readable summary.

---

# 9. Benchmark Matrix

## Particle capacity

Test:

- 1k;
- 10k;
- 100k;
- 500k;
- 1M.

## Occupancy

Test:

- 1%;
- 10%;
- 50%;
- 100%.

## Emitters per effect

Test:

- 1;
- 4;
- 16;
- 64.

## Effect instances

Test:

- 1;
- 10;
- 100;
- 1,000.

## Spawn mode

Test:

- burst;
- constant rate;
- curve;
- continuous looping.

## Curve complexity

Test:

- no curve;
- 2 keys;
- 4 keys;
- 8 keys;
- runtime-modified curve.

## Particle motion

Test:

- none;
- velocity only;
- gravity;
- drag;
- turbulence/noise;
- combined forces.

## Loop pressure

```text
particle lifetime / effect duration
```

Test:

- 1;
- 4;
- 16;
- 64.

## Renderer count

Test:

- 1 renderer per emitter;
- 2 renderers;
- 4 renderers.

## Materials

Test:

- all shared;
- several shared variants;
- one unique material per emitter;
- one unique material per effect.

## Runtime parameter mutation

Test:

- no changes;
- one scalar changed per frame;
- transform changed;
- several parameters changed;
- all runtime parameters changed every frame.

## Nested effects

Test:

- no nesting;
- shallow nesting;
- wide nesting;
- deep nesting.

## Screen coverage

Test:

- tiny particles;
- moderate coverage;
- large/fullscreen particles.

This separates simulation bottlenecks from fragment/overdraw bottlenecks.

---

# 10. Metrics

Never use average FPS as the primary benchmark result.

Record frame time directly.

## CPU

Record:

```text
whole frame
Aestra total CPU
runtime advance
CPU/reference evaluation
render extraction
GPU input preparation
artifact construction/update
material preparation
buffer uploads
bind-group preparation
allocation count/frame
allocated bytes/frame
peak resident memory
```

## GPU

Record:

```text
Aestra total GPU time
simulation reset time
simulation time
render time
workgroup count
dispatch count
draw count
particle capacity
alive particles
occupancy
GPU buffer bytes
```

## Statistical output

For each timing metric record:

```text
median
p95
p99
maximum
standard deviation
```

Optional:

```text
minimum
mean
MAD
```

Frame-time tails are especially important for production games.

---

# 11. Normalized Performance Metrics

Absolute timings are machine-dependent.

Also report normalized values:

```text
CPU ns / active effect
CPU ns / 1k particle slots

GPU ns / 1k slots
GPU ns / 1k alive particles

bytes / particle slot
bytes / active effect

draws / emitter
dispatches / effect

occupancy %
```

These metrics are useful both to developers and eventually to artists.

---

# 12. Initial Performance Budgets

These values are engineering targets, not guarantees for every game.

For a 60 Hz game:

```text
whole frame budget              16.67 ms

Aestra CPU typical target       ~1.0 ms
Aestra CPU stress ceiling       ~2.0 ms

Aestra GPU typical target       ~2.0 ms
Aestra GPU stress ceiling       ~4.0 ms
```

The host game ultimately decides its own VFX budget.

Aestra should make it possible to understand whether an effect fits within such a budget.

---

# 13. Memory Profiling

Memory must be treated as a first-class performance dimension.

Record:

- CPU heap allocations;
- persistent CPU memory per effect;
- GPU particle buffers;
- alive/dead buffers;
- indirect buffers;
- uniform/storage buffers;
- texture/material memory;
- total GPU bytes per effect.

Verify that profiler memory calculations match the real GPU ABI.

Avoid exposing artist-facing memory estimates that are derived from logical particle attributes when the shipping GPU particle layout has different storage requirements.

---

# 14. CI Performance Regression System

Aestra already has native GPU CI infrastructure.

Extend it with:

```text
.github/workflows/performance.yml
```

Use two levels.

## PR CPU lane

Run deterministic microbenchmarks on normal CI where possible.

Initially:

- collect results;
- upload artifacts;
- compare against baseline;
- report regressions.

Later, fail CI for statistically stable regressions.

Possible threshold:

```text
>5-10% stable CPU regression
```

depending on benchmark variance.

## Native GPU lane

Run on a fixed self-hosted GPU machine.

Record:

- GPU model;
- driver version;
- OS;
- graphics backend;
- CPU;
- Aestra commit;
- Bevy/WGPU version.

Run the benchmark suite and compare against the latest accepted `main` baseline.

GPU gating should initially be advisory because GPU clocks introduce noise.

Once variance is characterized, consider an alert/failure threshold such as:

```text
>10-15% sustained GPU regression
```

for stable scenarios.

---

# 15. Benchmark Result Format

Example JSON:

```json
{
  "scenario": "instance_stress_100x1000",
  "commit": "...",
  "hardware": {
    "cpu": "...",
    "gpu": "...",
    "driver": "...",
    "backend": "vulkan"
  },
  "content": {
    "effects": 100,
    "emitters": 100,
    "capacity": 100000,
    "alive": 47231,
    "occupancy": 0.47231
  },
  "cpu": {
    "aestra_total_ms": {
      "median": 0.82,
      "p95": 0.91,
      "p99": 1.04
    }
  },
  "gpu": {
    "simulation_ms": {
      "median": 1.31,
      "p95": 1.38,
      "p99": 1.42
    },
    "render_ms": {
      "median": 0.71,
      "p95": 0.77,
      "p99": 0.81
    }
  }
}
```

---

# 16. Benchmark Baseline Scenarios

Create a small set of canonical scenarios that are easy to understand.

## B001 — Empty

No active Aestra effects.

Purpose:

- establish integration overhead.

## B002 — Single Small Effect

```text
1 effect
1 emitter
1k capacity
high occupancy
```

Purpose:

- baseline runtime overhead.

## B003 — Single Dense Effect

```text
1 effect
1 emitter
100k-1M capacity
100% occupancy
```

Purpose:

- raw particle throughput.

## B004 — Sparse Large Capacity

```text
1 effect
500k capacity
1% occupancy
```

Purpose:

- expose capacity-bound analytical simulation.

## B005 — Many Small Effects

```text
100-1000 effects
low particle count each
```

Purpose:

- expose per-instance CPU/GPU overhead.

## B006 — Many Emitters

```text
1 effect
64 emitters
fixed total particle capacity
```

Purpose:

- expose emitter lookup complexity.

## B007 — Loop Pressure

Short effect duration with long particle lifetime.

Purpose:

- expose analytical historical reconstruction.

## B008 — Curve Stress

Complex emission curves.

Purpose:

- measure inverse-curve evaluation cost.

## B009 — Material Stress

Many materials and renderer variants.

Purpose:

- measure render preparation and bind-group cost.

## B010 — Overdraw Stress

Large particles covering most of the screen.

Purpose:

- separate simulation performance from fragment/rendering cost.

---

# 17. Optimization Milestones

## Milestone 0 — Freeze a baseline

Before large runtime changes:

- implement benchmark executable;
- record benchmark results for current `main`;
- archive JSON;
- record hardware and driver;
- capture representative Tracy traces.

This gives all later optimization work a measurable reference.

---

## Milestone 1 — Add real instrumentation

Implement:

- Tracy CPU spans;
- allocation profiling;
- GPU timestamp queries;
- per-stage timing;
- benchmark JSON export.

No significant runtime redesign yet.

Goal:

> Be able to explain where one Aestra frame is spent.

---

## Milestone 2 — Remove obvious per-frame CPU waste

Refactor GPU effect creation/update.

Goals:

- no particle-vector creation during simple instance updates;
- persistent particle buffers;
- immutable compiled GPU prototype;
- mutable instance data separated;
- precompute transforms/invariant values;
- remove avoidable temporary allocations.

Benchmark before/after.

---

## Milestone 3 — Dirty-state GPU updates

Implement explicit invalidation.

Examples:

```text
DirtyTime
DirtyTransform
DirtyParameters
DirtyEmitterData
DirtyMaterial
DirtyPrototype
DirtyBuffers
```

Only upload/rebuild what changed.

Goal:

> Static effects should have extremely low CPU preparation overhead.

---

## Milestone 4 — Cache GPU resources

Ensure that resources such as:

- bind groups;
- pipelines;
- immutable material resources;
- static emitter data;

are recreated only when their dependencies change.

Benchmark instance-count scaling again.

---

## Milestone 5 — Analytical kernel scaling study

Run the full benchmark matrix.

Determine measured relationships between:

```text
capacity
occupancy
emitter count
loop pressure
curve complexity
```

and GPU simulation time.

This milestone decides whether the current analytical simulation is sufficient as the main gameplay runtime.

---

## Milestone 6 — Optimize analytical simulation

Potential optimizations, depending on measurements:

- eliminate linear emitter search;
- eliminate unnecessary dead-list bookkeeping;
- reduce global atomics;
- simplify spawn-time reconstruction;
- precompute curve lookup data;
- reorganize emitter slot ranges;
- specialize simple emitters;
- reduce branch divergence.

Do not implement blindly.

Every optimization must correspond to a measured bottleneck.

---

## Milestone 7 — Evaluate incremental gameplay backend

If the analytical kernel is not adequate for production gameplay workloads, prototype:

```text
persistent particles
alive list
dead/free list
spawn pass
update pass
compaction/culling
indirect rendering
```

Compare directly against analytical execution using identical effects.

Evaluate:

- dense effects;
- sparse effects;
- long-lived effects;
- short loops;
- large instance counts.

Preserve analytical execution for:

- editor seeking;
- scrubbing;
- deterministic reference;
- tests.

---

## Milestone 8 — Batch effect execution

If per-instance overhead is significant, evaluate:

- global particle arena;
- global effect table;
- global emitter table;
- batched compute passes;
- multi-effect dispatch;
- reduced bind-group switching;
- indirect/multi-draw strategies.

Primary benchmark:

```text
same total particle count
different number of effect instances
```

Goal:

> 100-1000 small effects should not be dramatically more expensive than a small number of large effects solely because of submission overhead.

---

## Milestone 9 — Editor performance feedback

Once metrics are trustworthy, surface useful values in Aestra Editor.

Examples:

```text
Particle capacity
Alive particles
Occupancy
CPU cost
GPU simulation cost
GPU render cost
VRAM
Draw calls
Dispatches
Overdraw warning
```

Potential warnings:

```text
Low occupancy
High loop pressure
Too many unique materials
High emitter count
Excessive overdraw
Runtime parameter updates forcing repacks
```

The editor should eventually help artists optimize effects before programmers have to profile them manually.

---

## Milestone 10 — AAA Performance Gates

Define supported hardware tiers and production scenarios.

Examples:

```text
Minimum PC
Recommended PC
High-end PC
Steam Deck / handheld
Integrated GPU
console-equivalent profile
```

Maintain permanent benchmark baselines.

Aestra release candidates should not ship with unexplained performance regressions.

---

# 18. Definition of "AAA-Ready Runtime"

Aestra should not claim AAA-grade runtime performance merely because it can display a large particle count.

The runtime should demonstrate:

## Predictable scaling

Developers can predict how cost changes with:

- particle capacity;
- alive particles;
- emitters;
- instances;
- materials;
- overdraw.

## Stable frame times

No recurring large p99 spikes from:

- allocations;
- resource creation;
- shader compilation;
- uploads;
- synchronization.

## Low idle cost

An effect with almost no active work should have almost no runtime cost.

## Large-instance-count support

Hundreds of concurrent VFX instances must not create excessive CPU/render submission overhead.

## Memory discipline

Persistent memory use must be measurable and bounded.

## Artist visibility

Expensive effects must be diagnosable from the editor.

## Automated regression detection

Performance regressions must be visible during development rather than discovered late in a game project.

---

# 19. Recommended Immediate Order

The recommended next implementation sequence is:

1. create `aestra-bench`;
2. add canonical benchmark scenarios;
3. add Tracy CPU/allocation instrumentation;
4. add real GPU timestamps;
5. capture the current baseline;
6. remove per-frame GPU artifact/particle allocation;
7. introduce dirty-state updates;
8. cache bind groups and immutable GPU resources;
9. run capacity/occupancy/emitter/instance scaling experiments;
10. optimize measured analytical-kernel bottlenecks;
11. decide whether an incremental gameplay backend is required;
12. prototype and compare it if necessary;
13. investigate global batching only after instance-count benchmarks justify it;
14. add performance CI regression reporting;
15. expose stable metrics to the editor.

---

# 20. Main Architectural Principle

The most important rule for this work is:

> **Do not optimize Aestra toward a synthetic particle-count record. Optimize it toward predictable, bounded frame-time cost in real game scenes.**

A production VFX runtime must handle both:

```text
one huge effect
```

and:

```text
hundreds of small independent effects
```

while keeping:

- CPU cost;
- GPU cost;
- allocations;
- memory;
- draw/dispatch overhead;
- frame-time variance;

visible and controllable.

The benchmark framework should therefore become a permanent part of Aestra's architecture, not a one-time optimization exercise.
