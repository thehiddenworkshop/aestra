# Aestra — Realistic Fireworks Showcase: Current State & Implementation Roadmap

**Repository snapshot reviewed:** uploaded project snapshot `aestra-main(1).zip` (2026-10-03)
**Review date:** 2026-10-03
**Goal:** make Aestra capable of authoring and rendering a convincing, reusable, multi-style professional fireworks show without introducing firework-specific runtime concepts.

---

## 1. Executive summary

Aestra is now far enough along that realistic fireworks should move from a future architecture benchmark to an **active reference showcase**.

Several features that were previously the largest blockers are already implemented in the current codebase:

- GPU particle simulation.
- Persistent/stateful particle simulation.
- GPU checkpoints and deterministic replay/seeking.
- Particle event links / sub-emitters.
- `OnSpawn`, `OnDeath`, and `OnCollision` particle events.
- Multiple renderer instances per emitter.
- Sprite, flipbook, mesh, ribbon, and proper history-based trail renderers.
- Trail replay/checkpointing, adaptive sampling, compaction, culling, bounds and profiling infrastructure.
- Timeline regions and nested reusable `EffectClip` composition.
- Curves, gradients, random scalar ranges, gravity, drag, turbulence and particle appearance controls.
- Extensible stage/domain architecture.
- A substantial external fluid extension with gas/liquid simulation and volumetric presentation.

The architecture provides the *functional* backbone of a real shell, but current event and trail implementations cannot execute the proposed hero and finale workloads at authored scale:

```text
Rocket
  ├─ persistent ballistic motion
  ├─ bright head
  └─ launch trail
       │
       └─ OnDeath
            ↓
        Main burst
          ├─ hundreds of stars
          ├─ sprite heads
          ├─ persistent trails
          └─ OnDeath
               ↓
             crackle / secondary burst
```

The main gaps have changed. They are no longer "Aestra needs a stateful particle system" or "Aestra needs trails". The important remaining work is now:

1. **Scalable, deterministic event collection and burst expansion**, plus event-link authoring.
2. **Scalable trail ownership, history update, compaction and memory layout.**
3. **General velocity/spawn distribution authoring:** F2's six native velocity modes are implemented end-to-end; visual shell acceptance remains separate from production readiness.
4. **HDR/exposure/bloom support in the editor preview.**
5. **Scene lighting:** representative event-bound lights are implemented; editor authoring, direct particle lighting and lit smoke remain.
6. **Better smoke integration, eventually including particle → fluid injection.**
7. **Measured performance/LOD behavior under finale-scale workloads.**
8. **A generic host-facing cue API** so the example host can bind real particle events to spatial sound and show control.
9. **A reusable library of actual firework effects and a show-level authoring workflow.**

Implementation update (2026-10-03): F2/F3 prototypes and F4's reference hero are implemented;
F4 artistic acceptance remains open. **F5A validates a bounded two-generation death-event
shell; F5B validates particle-driven host cues and seek/restart delivery epochs; F5C/D add
bounded delayed-carrier crackle and four-arm crossette; F5E adds independently phased
strobe stars with exact off intervals. F5F adds explicit tier-specific secondary-shell
budgets and measured overlapping forward playback. F6A adds a bounded 26-second reusable
show with per-clip admission/cleanup reporting across all three tiers. F6B validates
nested spatial particle cues at all three tiers, including seek/restart suppression.
F7A adds engine-neutral host-submitted light pulses, a bounded opt-in Bevy pool and
native show/receiver acceptance. F7B1 persists generic light bindings through source,
compiler and artifact v6 and resolves them automatically in Bevy. A fresh code review
of this snapshot confirms that these bindings are intentionally **representative event
lights**, not lights continuously emitted by every particle. F7B2 now has editor
authoring controls (manual visual acceptance remains open). **F7C now implements the material-free
particle scene-output model, compiled plans and deterministic CPU/reference candidates, persisted in
artifact v7. F7D1 now has a measured portable per-output GPU selection prototype;
F7D2A adds global admission and opt-in live GPU presentation wiring, validated against overlapping
stateful launch-to-star event chains. F7D2B now measures the current authored hero, overlapping volley
and F6 show with ordinary material/trail rendering intact across all tiers. Production-finale
certification, live selected-light realization and lit smoke remain open.**
The next engine step is F7E's bounded asynchronous selected-light realization and latency/receiver gate,
not more unmeasured F1B micro-optimization. Return to scalability work when authored hero/finale
workloads expose a measured blocker. Do not lower the hero-shell target or work around the
engine with duplicated links or emitters. F1/F1B's open performance/resource gates still
precede a production-shell claim, not merely F9 polish.

---

# 2. Scope and principles

## 2.1 Target visual result

The target is not merely:

```text
rocket → radial sprite explosion
```

The target is a show that, at normal viewing distance, has the major cues of filmed professional fireworks:

- physically plausible launch and ballistic motion;
- bright HDR cores;
- persistent incandescent trails;
- gravity and drag;
- characteristic shell geometries;
- delayed sub-bursts and crackles;
- color changes through burn phases;
- variation between nominally identical shells;
- accumulating smoke;
- smoke illuminated by later shells;
- large transient flashes that affect the environment;
- multiple launch sites and synchronized volleys;
- launch, burst and crackle sounds synchronized to actual simulation events by the example host;
- dense finales without catastrophic performance collapse;
- timeline scrubbing/replay that remains deterministic and usable.

## 2.2 Architectural principle

**Do not add `FireworkShape`, `FireworkEmitter`, `FireworkRenderer`, or a firework-specific runtime.**

A firework should be authored from generic Aestra concepts:

```text
EffectClip
Emitter
Stage
Module
Distribution
EventLink
Renderer
Material
Curve / Gradient
Host Binding
Extension Domain
```

If a feature is useful only because fireworks need it, ask whether it can be generalized first.

Examples:

- Ring shells need a ring velocity distribution → implement generic directional/velocity distributions.
- Crossettes need delayed secondary spawning → improve generic event triggering.
- Bright bursts need bloom → improve the generic preview render pipeline.
- Smoke illumination needs transient light output → implement generic scene-output/light integration.

The fireworks project should therefore act as a **system-level acceptance test for Aestra**.

---

# 3. Current codebase state

## 3.1 Stateful particle simulation — implemented

The Bevy GPU renderer contains persistent state infrastructure in:

```text
bevy/aestra-bevy-render/src/gpu.rs
```

including `StatefulPersistentState` and GPU checkpoint logic.

Current constants include:

```rust
const STATEFUL_CHECKPOINT_CADENCE: u32 = 20;
const MAX_STATEFUL_CHECKPOINTS: usize = 64;
```

Persistent simulation stores particle state such as position, velocity, age/lifetime and allocator/free-list state across fixed ticks.

### Why this matters for fireworks

A realistic firework quickly becomes history-dependent:

- drag;
- turbulence;
- trails;
- collisions;
- secondary spawning;
- future plugin simulation;
- arbitrary staged updates.

This work means Aestra no longer needs to fake every shell analytically.

### Current assessment

**Status: strong foundation.**

Keep the hybrid model. Do not replace all stateless simulation with stateful simulation just for fireworks.

---

# 4. Seeking and deterministic replay — implemented

Stateful emitters have GPU checkpoints and replay support. Trail infrastructure has its own replay/checkpoint handling as well.

This is particularly important because a fireworks editor without good seeking would be unpleasant to author. A 30-second sequence must support repeatedly moving around:

```text
12.0 s → 18.0 s → 14.5 s → 29.0 s
```

without requiring the entire show to restart visibly from zero each time.

### Current assessment

**Status: one of Aestra's important differentiators.**

Fireworks should become a stress test for:

- seeking into dense stateful periods;
- restoring nested `EffectClip` instances;
- restoring trails;
- restoring chained event-generated particles;
- preserving deterministic seeds.

---

# 5. Particle events / sub-emitters — implemented

The core model defines:

```rust
pub enum EventTrigger {
    OnSpawn,
    OnDeath,
    OnCollision,
}
```

and:

```rust
pub struct EventLink {
    pub source: EmitterId,
    pub trigger: EventTrigger,
    pub target: EmitterId,
    pub count: u32,
    pub inherit_velocity: f32,
    ...
}
```

Semantics are already exactly in the direction fireworks need:

> when a particle in the source emitter triggers the event, spawn particles from a target emitter at that position, optionally inheriting source velocity.

The runtime compiles those links and has GPU execution/conformance coverage **within one compiled effect**. A link cannot directly target an emitter inside a different reusable shell asset or `EffectClip`; show-level composition does not change that scope.

### This enables

#### Standard burst

```text
Rocket particle
    │
  OnDeath
    ↓
Star emitter
```

#### Pistil shell

```text
Rocket
 ├─ OnDeath → Outer stars
 └─ OnDeath → Inner stars
```

#### Crossette

```text
Primary star
    │
  OnDeath
    ↓
4 child stars
```

#### Crackle

```text
Burning star
    │
  OnDeath
    ↓
crackle spark cluster
```

#### Multi-break shell

```text
Break 1
   ↓
carrier stars
   ↓ OnDeath
Break 2
```

### Current assessment

**Runtime semantics: implemented for bounded workloads.**
**Authoring: incomplete.**
**Hero/finale scale: blocked by the current event pipeline.**

---

# 6. Event scale is a prerequisite, not a constant change

The core model currently enforces:

```rust
pub const MAX_EVENT_LINK_COUNT: u32 = 64;
```

and validates each event link to between 1 and 64 particles per triggering event.

The runtime has a separate capacity:

```rust
pub const PARTICLE_EVENT_CAPACITY: u32 = 1024;
```

which bounds source events per emitter/tick and spawned particles per link/tick. The Bevy gather path allocates both buffers at that fixed capacity. Its shader ranks each matching event by scanning all source events, then writes up to the list capacity; this is quadratic in source-event count before multiplying by children. The list-capacity exit does not itself report how many children were discarded. Raising `MAX_EVENT_LINK_COUNT` alone would make the authoring UI accept a burst that the execution path could silently truncate.

These are different limits at different layers; all must be addressed together.

## Why 64 is problematic

A convincing large shell can easily want hundreds of visible stars:

```text
small shell          ~50–150 visual stars
medium shell         ~150–400
large shell          ~300–1000+
```

The exact number depends on rendering/trail strategy, but 64 is unnecessarily restrictive for authoring.

It can technically be bypassed with several links:

```text
Rocket OnDeath
 ├─ Stars A ×64
 ├─ Stars B ×64
 ├─ Stars C ×64
 └─ Stars D ×64
```

but that is poor authoring UX and exposes an implementation limit to artists.

## Required engine change

Separate four quantities:

1. **Authored count per triggering event** (`EventLink.count`, still a deterministic integer initially).
2. **Source-event demand per tick** (including chained events and coincident deaths).
3. **Expanded child demand per link and target per tick** (matching events × count, summed across links to a target).
4. **Physical GPU capacity** (buffers, destination particle slots, dispatch limits and memory budget for the selected device/quality tier).

Implement the scale path before increasing the authored validation range:

- Plan capacities using checked arithmetic and the compiled graph's emitter capacities and link multiplicities. Validate device limits and report the estimated peak memory/dispatch cost. Where a static worst case is too conservative, allocate/grow within an explicit per-effect budget or process bounded chunks; a single fixed 1,024-entry buffer is not an acceptable universal ceiling.
- For each tick, estimate link demand as `matching_source_events × authored_count`, then sum incoming link demand per target. Use checked wide integers for these calculations and include simultaneous effect instances in the host-level budget.
- Replace the per-event scan for ordinal rank with a parallel, deterministic classify/compact/order/expand pipeline. Preserve source-ordinal and child-index ordering across workgroups and chunks; do not rely on atomic append order for replay.
- Batch or dispatch child expansion without requiring every child's intermediate record in one monolithic storage binding. Account for fan-out from several links and several simultaneous shells, not only one rocket death.
- Define one overflow contract for source-event capture, per-link expansion and destination-emitter slots. Preferred behavior is compile-time rejection of guaranteed over-budget graphs and a stable, documented runtime selection policy for variable peaks. Counters must report requested, emitted and dropped quantities by effect/emitter/link, never merely disappear at the buffer bound.
- Keep resource budgets finite and device-aware. Remove arbitrary small global caps from the authoring model; do not promise infinite allocation. Quality tiers may intentionally reduce child counts, but the effective count must be visible and deterministic.

A random integer `count` distribution is a separate later authoring feature. It must not be used as a shortcut around the fixed-buffer problem.

### Acceptance gate

- A single `OnDeath` link authors and executes a 300–800-star hero burst without duplicate links or lost children.
- Tests cover many source events on the same tick, chained links, multiple links into one target, and an over-budget case at every queue/target boundary.
- Fixed seeds produce the same ordered child identities and state after restart, checkpoint restore and forward/backward seek; CPU reference and GPU behavior agree where the reference supports the workload.
- Profiling reports event collection, ordering, expansion, buffer memory and any drops. The event path meets an explicit per-tier GPU-time budget on named test hardware before proceeding to the shell library.

---

# 7. Event-link editor UI is behind the runtime

The current editor properties code displays event links but does not expose their important runtime properties.

`EditorSession::add_event_link()` currently creates:

```rust
count: 1,
inherit_velocity: 0.0,
```

The Properties UI primarily offers trigger/target creation and deletion.

There is also stale localization/UI text:

```text
Event links are saved but do not trigger particles in the current runtime.
```

while the current runtime clearly contains production GPU event execution.

## Required editor design

Selecting an event link should expose at least:

```text
EVENT LINK
────────────────────────────
Source            Rocket
Trigger           On Death
Target            Main Stars

Spawn Count       380
Inherit Velocity  0.00

[ Delete Link ]
```

Later:

```text
Spawn Count       Random Integer / Distribution
Position Offset   ...
Velocity Scale    ...
Seed Policy       ...
Condition         ...
```

## Commands required

Introduce semantic authoring commands rather than editing the model directly, e.g.:

```text
SetEventLinkCount
SetEventLinkInheritVelocity
SetEventLinkTrigger
SetEventLinkTarget
```

with undo/redo and validation.

## Acceptance criteria

- Create a Rocket → Stars `OnDeath` link entirely from UI.
- Set count to a high burst value.
- Set inherited velocity.
- Undo/redo every property.
- Save/reload and preserve values.
- No stale unsupported-runtime warning remains.

---

# 8. Trails — functional foundation, scale gate still required

Trail rendering is no longer a missing primitive.

The model supports properties including:

```text
width
sample_interval
lifetime
max_points
max_trails
sampling mode
sample_distance
curve_tolerance
UV mode
tile length
end-cap mode
```

The GPU system contains dedicated infrastructure such as:

```text
crates/aestra-gpu/src/shaders/aestra_trail_history.wesl
crates/aestra-gpu/src/shaders/aestra_trail_geometry.wesl
crates/aestra-gpu/src/shaders/aestra_trail_bounds.wesl
crates/aestra-gpu/src/shaders/aestra_trail_compact.wesl
crates/aestra-gpu/src/shaders/aestra_trail_cull.wesl
crates/aestra-gpu/src/shaders/aestra_trail_vertex.wesl
```

and Bevy-side replay, compaction, culling, bounds and conformance tests.

## Why this is essential

The primary visual feature of many real shells is not the star head but its persistent burning trajectory.

### Chrysanthemum

```text
       \  |  /
        \ | /
     ---- * ----
        / | \
       /  |  \
```

### Willow

```text
      ╭────╮
    ╭─╯    ╰─╮
   ↓          ↓
```

A stretched sprite is insufficient for those looks. A history-based trail is the correct primitive.

## Current hard limits and cost

F1B has removed the **1,024-parent/owner implementation ceiling**. Small pools retain the cooperative 64-lane path; larger pools use storage-backed page sorts/merges, stable-ID lookup, survivor reservation, deterministic birth allocation, and parallel history/bounds updates. Native conformance exercises 8,192 parents with 16,384 owners, including real event-born trajectories. Samples remain bounded to **2–64 points per owner**, and the GPU artifact to **1,048,576 total particle/history records**, with additional checked scratch, buffer-binding and dispatch limits from the adapter. Workgroup memory remains within the portable 16 KiB budget and the simulation still uses eight storage bindings.

Logical owner pages do not yet split a physical buffer across bindings. The total-record ceiling and oversized-binding rejection remain genuine limits, not quality-tier policies. Full overlapping-shell/finale performance certification is still open; an 8,192-parent workload passing correctness is not an unlimited-scale claim.

`max_trails` must also account for retired tails. A parent dying does not immediately free its visible history; active parents plus unexpired tails can exceed the parent-particle count.

## Required engine change

- Replace repeated parent × owner search with a stable particle-ID-to-owner mapping and a deterministic owner allocation/retirement policy. Keep history attached to its parent identity through particle-buffer compaction and checkpoint restore.
- Update and expire owner histories in parallel, then reduce per-owner bounds into emitter/effect bounds. Preserve time- and distance-sampling semantics, tail lifetime, eviction telemetry and replay behavior.
- Replace single-invocation owner prefixing with a parallel deterministic scan/compaction path. Preserve the ordering contract needed by alpha drawing, or define and test an explicit alternative for blended trails.
- Plan history, scratch, draw and checkpoint storage from trail-owner and point budgets with checked arithmetic. Use chunked/paged resources where one binding would exceed the device's limits. Budget active and retired tails separately; expose allocated, occupied, retired, evicted and truncated counts.
- Retain finite device/quality-tier budgets. The 256/1,024-parent and owner implementation ceilings are removed; certify large-pool performance and physical resource chunking before calling finale workloads supported. Revisit the 64-point cap through measured long-trail quality and memory tests, not by assuming every trail needs more points.

## Acceptance gate

- One emitter compiles and replays a 300–800-star hero shell with a corresponding trail for every star, without emitter splitting or forced trail loss.
- Separate stress tests cover thousands of concurrent histories and the finale target of thousands to tens of thousands across overlapping shells; active plus retired tails are counted.
- Fixed-seed trail geometry, bounds, compaction and eviction are repeatable after checkpoint restore and reverse seek.
- GPU time, memory, visible/culled primitive counts and overflow/eviction counters are recorded on named hardware. The scene remains interactive under an explicit quality-tier budget; budget reductions are visible rather than silent.

**Current assessment:** analytic and event-born/stateful 800-parent single-emitter bursts compile and render matching histories. Native GPU tests cover paged 8,192-parent pools, retention and synchronized restoration. A budgeted repeated-cohort volley now proves complete admission and retention through overlap and drain; full-frame percentile budgets and a complete finale remain unproven. The complete hero/finale gate is still **open**.

---

# 9. Multiple renderer instances — implemented

Emitters contain:

```rust
pub renderers: Vec<RendererInstance>
```

and support renderer types including:

```text
Sprite
Flipbook
Ribbon
Trail
Mesh
Custom
```

This is the right model for firework stars.

Example:

```text
Star Emitter
├─ Sprite Renderer
│    tiny white-hot core
│
├─ Sprite Renderer
│    softer colored glow
│
└─ Trail Renderer
     long incandescent path
```

No renderer should need to become a special "firework renderer".

### Current assessment

**Ready.**

---

# 10. Current spawn shapes are useful but not enough

Aestra already has emitter position shapes:

```rust
Point
Circle
Ring
Sphere
Hemisphere
Box
Cylinder
Cone
```

This is good for **where particles begin**.

However, current initialization fundamentally uses:

```text
speed range
base direction
angular spread
```

The runtime samples direction inside a cone/solid angle around a forward direction.

That is not the same as a general **velocity distribution** system.

## Why this matters

### Peony

A near-spherical velocity distribution works well.

```text
      ↖ ↑ ↗
    ←   *   →
      ↙ ↓ ↘
```

This is already approximately achievable using a very wide spread.

### Ring shell

A ring needs velocities constrained to a plane:

```text
       ↖   ↗
    ←    *    →
       ↙   ↘
```

with almost no velocity perpendicular to the ring plane.

### Palm

A palm wants a small number of controlled branch directions with variation.

### Heart / star / smiley shell

These need sampling from a defined directional point set / curve / mesh / field.

The current `direction + spread` model becomes awkward or impossible for these.

---

# 11. Required generic distribution architecture

This is the highest-priority new simulation/authoring feature for fireworks.

Do not encode it as a firework option.

## 11.1 First target

Introduce a generic direction/velocity distribution concept.

Conceptually:

```text
Initialize Velocity
│
├─ Direction Distribution
│   ├─ Constant
│   ├─ Cone
│   ├─ Sphere
│   ├─ Hemisphere
│   ├─ Disk
│   └─ Ring
│
└─ Speed
    ├─ Constant
    └─ Range
```

Important: position and velocity distributions should remain independent.

For example:

```text
position = Point
velocity = Ring
```

creates a ring shell.

Whereas:

```text
position = Ring
velocity = NormalFromPosition
```

means something quite different.

## 11.2 Likely later forms

```text
Direction Distribution
├─ From Position / Radial
├─ Toward Point
├─ Away From Point
├─ Cone
├─ Sphere Surface
├─ Hemisphere Surface
├─ Disk
├─ Ring
├─ Along Spline
├─ From Mesh Vertices
├─ From Mesh Surface
├─ From Texture / point cloud
├─ Sample Vector Field
├─ Expression
└─ Custom extension
```

## 11.3 Distribution should be extensible

This should fit Aestra's namespaced type/plugin model.

A plugin should eventually be able to contribute a distribution without modifying core enums everywhere.

Short-term built-ins can still be native, but avoid painting the architecture into an enum-only corner if possible.

## 11.4 Determinism contract

Every distribution must define deterministic sampling from:

```text
particle stable id
emitter seed
instance/effect seed
sample dimension/channel
```

The result must remain stable across:

- replay;
- checkpoint restore;
- CPU reference implementation;
- GPU implementation;
- nested `EffectClip` instances.

## 11.5 Acceptance effects

The distribution system is sufficient when these can be authored without custom runtime code:

- Peony.
- Ring.
- Palm.
- Hemisphere/fan.
- Double ring with two emitters.

---

# 12. Event triggers should eventually become richer

Current triggers:

```text
OnSpawn
OnDeath
OnCollision
```

are enough for the first serious showcase.

However, fireworks will quickly motivate more expressive triggers.

## Useful future triggers

```text
OnAge(seconds)
OnNormalizedAge(0..1)
OnAttributeThreshold
OnCondition
OnEnterVolume
OnExitVolume
```

### Example

A burning star should split at 65% life while the original continues:

```text
Star
  │
  ├─ at 65% age → spark split
  │
  └─ continues burning
```

Using `OnDeath` forces the parent star to end just to trigger the next effect.

## Recommended architecture

Avoid expanding `EventTrigger` forever with special cases.

Longer-term move toward:

```text
Event Trigger
├─ lifecycle trigger
├─ predicate/condition
└─ emitted event payload
```

But **do not block the first fireworks milestone on this**.

---

# 13. HDR, exposure and bloom — implementation foundation landed; artistic acceptance remains

The original gap described here has substantially closed. F4A–F4M later in this roadmap
implement a shared HDR viewer/editor response, exposure, tonemapping, bloom, authored HDR
radiance controls, sampling/culling work and a reference-driven hero shell.

The remaining gate is primarily **artistic/reference acceptance and production tuning**, not
the absence of an HDR pipeline. The semantic material distinction between base color and
emitted radiance is still worth revisiting if future lit-material integrations require it.

## Why bloom matters

A firework is a luminous source several orders of magnitude brighter than much of the night scene.

If all color is clipped to display white before post-processing, the result looks like flat glowing sprites.

Instead, material output should be able to carry energy such as:

```text
[12.0, 3.0, 0.5]
```

and the camera pipeline should map this through exposure, bloom and tonemapping.

## Required preview controls

At minimum:

```text
PREVIEW / POST PROCESS
────────────────────────
HDR               On
Exposure          0.0 EV
Tonemapper        <choice>
Bloom             On
Bloom Intensity   ...
Bloom Threshold   ...
```

Advanced options can remain hidden by default.

## Material semantics

Aestra currently has material/output concepts that can create bright-looking color, but the semantic distinction between ordinary base color and emitted light should be reviewed.

Longer-term consider a semantic material output such as:

```text
Color
Alpha
Emissive
Vertex Offset
```

rather than making authors encode emission implicitly into color.

This is especially important if future render integrations need to distinguish surface albedo from radiance.

## Acceptance criteria

A single gold star should show:

- small hot white/yellow core;
- colored bloom halo;
- bright additive overlap when stars cross;
- stable exposure while the burst appears;
- no severe clipping/banding in the preview.

---

# 14. Scene lighting — representative lights implemented; direct particle lighting is the next extension

The current snapshot has a real, generic scene-light path. This changes the previous assessment
that scene lighting was simply missing.

## 14.1 What is implemented now

Core/runtime:

```text
EffectAsset::point_lights
    ↓
PointLightBinding
    ↓ stable FirstPerTick particle-output route
PointLightPulse
    ↓
TransientPointLight
```

`PointLightPulse` is engine-neutral and carries:

- normalized linear RGB;
- lumen intensity curve;
- range curve;
- source radius;
- independent pulse duration.

`PointLightBinding` is persisted on the effect and references a stable particle-output route.
It can optionally resolve its color from an effect gradient parameter at an authored normalized age.
The compiler lowers the binding into a route index and optional parameter slot, and artifact v6
persists it.

The Bevy adapter provides an opt-in `AestraTransientLightPlugin` that realizes these intents with a
bounded pool of shadowless `PointLight` entities. Current defaults/portable ceilings are deliberately
finite:

```text
Default active pool       16
Portable active ceiling   64
Default requests/frame    128
Portable request ceiling  1024
```

It also provides:

- global enable/disable;
- lumen/range clamps;
- occurrence-time decay rather than delivery-time restart;
- root epoch/seek/restart invalidation;
- duplicate/stale/invalid/budget telemetry;
- render-layer propagation;
- entity reuse rather than unbounded spawning.

The current firework sources save representative burst bindings and the viewer no longer contains a
firework-specific light mapper. Fresh receiver captures demonstrate real diffuse scene response with
bloom disabled.

### Important semantic correction

Although the binding is stored on `EffectAsset`, it is not simply a light fixed to the effect origin.
It is bound to a **particle output route** and uses the selected event packet's world position. It is
best described as:

> **one representative transient scene light generated by a particle event**.

This is exactly right for a shell burst flash, impact flash, explosion or other sparse/high-energy
VFX event.

## 14.2 What the current path intentionally does not do

The current contract explicitly rejects `EachEvent` routes for authored light bindings. It does not
create one light for every particle and does not maintain a continuously moving light attached to a
particle.

Current representative-light semantics are therefore:

```text
Rocket / star event
       ↓ FirstPerTick
PointLightBinding
       ↓
one transient pulse at the event position
```

not:

```text
Every alive luminous particle
       ↓ every frame
moving scene light
```

Other current limitations are intentional and correctly documented in the code:

- no per-particle identity contract through this host-event path;
- no future scheduling;
- no light-history reconstruction after seek;
- no moving attachment after emission;
- shadows/contact shadows disabled;
- current particle smoke is unlit;
- production lighting cost/finale scale is not yet certified.

Do **not** weaken this representative path to make it serve direct particle lights. It has useful,
clean event semantics and should remain.

## 14.3 Why direct particle lighting is still useful

Representative burst lighting gets most of the large-scale illumination of a firework, but a high-end
night scene can benefit from a small number of moving local lights selected from luminous particles:

```text
500 visible stars
├── 500 HDR sprites
├── 500 trails
├── 1 strong representative burst flash
└── 16–64 selected moving particle lights   ← new optional path
```

The same generic capability is useful for:

- embers;
- fireflies;
- magic projectiles;
- electric fragments;
- molten debris;
- candle/fire clusters;
- energy particles.

It must be considered an optional presentation feature. Every luminous particle should still look
correct from HDR radiance/bloom when particle lighting is disabled.

## 14.4 Do not implement this as one Bevy `PointLight` per particle

A direct mapping such as:

```rust
for particle in particles {
    spawn(PointLight { ... });
}
```

would violate Aestra's GPU-first architecture and scale poorly for fireworks or thousands of flames.
The correct model is:

```text
all luminous particles
       ↓
GPU candidate evaluation
       ↓
compaction / scoring / budget
       ↓
small selected light set
       ↓
backend realization
```

Simulation count and light count must be independent.

## 14.5 Code-specific design constraint: today's RendererInstance always requires a material

The obvious UI name for this feature is “Light Renderer”, and Niagara uses that terminology. However,
Aestra's current renderer model is structurally material-backed:

```text
RendererInstance
  id
  renderer_type
  enabled
  material              ← mandatory
  properties
```

Validation rejects a nil material. Built-in `RendererPlan` values carry a material, and even
`CompiledExtensionRenderer` carries a material structurally. A light has no raster material.

Therefore **do not add `RendererProperties::Light` with a dummy material** merely to make the UI say
“renderer”. That would encode a false invariant and make plugin/serialization semantics less clear.

### Recommended model

Introduce a sibling presentation concept, tentatively:

```text
Emitter
├── Modules
├── Renderers
│   ├── Sprite
│   ├── Trail
│   ├── Ribbon
│   └── Mesh
└── Scene Outputs / Presentation Outputs
    └── Particle Point Light
```

Possible engine-neutral shape:

```text
SceneOutputInstance
  id
  output_type
  enabled
  properties

SceneOutputProperties::ParticlePointLight {
    color_source,
    intensity,
    range,
    radius,
    selection,
    max_lights,
    shadow_policy,
}
```

The exact names should be chosen alongside the wider scene-output architecture. The important property
is that the object is **particle-backed but not material-backed**.

A future refactor could generalize renderers and scene outputs under a common presentation-sink
registry if that produces a real benefit, but direct particle lighting should not force an invasive
`material: Option<MaterialId>` migration first.

## 14.6 Reuse the existing particle presentation ABI

The current GPU presentation record is already a strong input for light selection:

```text
GpuParticle
  color: vec4
  position: vec3
  size: f32
  rotation: f32
  normalized_age: f32
  packed_emitter_alive: u32
  particle_index: u32
```

Stateful presentation writes the stable spawn ordinal into `particle_index`. That gives the light
selection path:

- current interpolated position;
- current particle color/opacity;
- size;
- normalized age;
- emitter identity;
- stable per-particle identity for deterministic tie-breaking.

A first particle-light implementation therefore does **not** need to mutate particle simulation state
or add light-specific data to every stateful record.

If later effects require independent per-particle light intensity/range unrelated to existing
appearance, add optional authored curves/expressions to the scene-output definition before expanding
the base particle ABI.

## 14.7 Proposed two-path lighting architecture

Keep both paths because they solve different scales of the problem:

```text
                         AESTRA LIGHTING

      sparse high-energy event                continuous particles
                │                                   │
                ▼                                   ▼
      PointLightBinding                    Particle Light Output
      FirstPerTick route                   current alive particles
                │                                   │
                ▼                                   ▼
      TransientPointLight                  GPU candidate buffer
                │                                   │
                │                           compact / score / top-K
                │                                   │
                └──────────────┬────────────────────┘
                               ▼
                         backend lighting
```

### Path A — representative transient light

Use for:

- shell burst flash;
- explosion;
- impact;
- large lightning flash;
- one/few scene lights with independent envelopes.

This is already implemented and should remain event-driven.

### Path B — bounded particle light output

Use for:

- moving luminous stars;
- embers/fireflies;
- selected sparks;
- local energy particles.

This is continuous presentation derived from alive particle state and must **not** route through host
`EachEvent` messages every frame.

## 14.8 Particle-light authoring model

A first authoring surface could expose:

```text
PARTICLE POINT LIGHT
────────────────────────────
Enabled              On

Color Source
  Particle Color

Intensity
  2,500 lm
  × Curve Over Life

Range
  5.0
  × Curve Over Life

Source Radius
  0.05

Selection
  Brightest / Relevant

Maximum Lights
  High       64
  Medium     16
  Low         0

Shadows
  None
```

The exact defaults must be benchmarked rather than treated as universal values.

Useful color sources:

```text
Particle Color
Constant
Parameter / Gradient
```

Useful intensity/range sources initially:

```text
Constant
Curve over particle life
Parameter × curve
```

Do not add a general expression graph merely for the first light implementation.

## 14.9 Candidate generation and selection

Particle-light cost must be explicitly bounded.

Recommended GPU flow:

```text
alive particle presentation records
        ↓
light-enabled / intensity evaluation
        ↓
candidate compaction
        ↓
importance score
        ↓
stable top-K selection
        ↓
selected light buffer
```

Candidate score can consider presentation-only information such as:

- intensity;
- projected size;
- distance to camera;
- screen visibility;
- authored priority.

Because selection is presentation-only, camera-dependent selection does not need to alter simulation
state or checkpoints. However, given the same frame/camera/settings the selection should be stable;
use `particle_index` as a deterministic tie-break rather than atomic append order.

## 14.10 Backend realization strategy

There are two reasonable Bevy implementation levels.

### First integration / validation path

Reuse the existing bounded transient-light pool, but feed it a bounded selected set rather than one
message per simulated particle.

For GPU particles this requires **asynchronous bounded GPU readback** of only the selected records.
Never synchronously read back the full particle buffer. Any latency must be measured visually on fast
moving sparks and recorded as a limitation.

This path minimizes integration risk and lets Aestra validate:

- authoring;
- GPU selection;
- budgets;
- effect quality;
- ordinary mesh lighting.

### Production path

If bounded readback/pool updates cause visible lag or material frame cost, integrate the selected
particle-light buffer directly with the backend's render-world/clustered-light path.

That code is inherently backend-specific:

```text
Aestra generic selected-light contract
        ↓
Bevy GPU realization
Godot realization
future Unity/Unreal realization
```

Do not put Bevy light structures into `aestra-core`.

## 14.11 Shadow policy

Direct particle lights should default to **no shadows**.

If shadows are added later, use a separate tiny budget:

```text
None                         default
Selected / Important         e.g. top few only
Backend Advanced             optional
```

The strong representative burst light is the better candidate for expensive scene shadowing than
hundreds of spark lights.

## 14.12 Quality/LOD policy

Lighting quality should degrade independently from particle density.

Example strategy:

```text
High
  representative burst lights: on
  particle lights: bounded high set

Medium
  representative burst lights: on
  particle lights: smaller set

Low
  representative burst lights: small pool
  particle lights: off

Very distant
  representative/proxy lighting only
```

This is particularly important for thousands of candles or a fireworks finale. A future spatial
aggregation path can merge many distant luminous particles into representative cell/cluster lights,
but it is not required for the first firework implementation.

## 14.13 Acceptance criteria for direct particle lighting

Do not call the feature complete until all of these hold:

- a moving luminous particle can visibly illuminate nearby ordinary scene geometry;
- selection follows particle motion without obvious popping beyond the authored/budgeted policy;
- 500–1000 visible firework stars do not create 500–1000 host light entities;
- hard light count remains bounded at all quality tiers;
- disabling particle lights does not change particle simulation or seeking;
- stable replay/capture with the same camera/settings selects the same lights;
- representative burst lights and particle lights can coexist without duplicate/unbounded energy;
- profiler exposes candidate count, selected count, dropped count and realization cost;
- finale-scale performance is measured rather than inferred;
- particle lights can be disabled globally by the host.

## 14.14 Firework target after this extension

The reference shell should eventually use:

```text
Chrysanthemum
├── Rocket
│   ├── HDR sprite
│   ├── trail
│   └── optional tiny particle light
│
├── Main burst event
│   └── representative PointLightBinding
│       strong, short, large-range flash
│
└── Stars
    ├── HDR sprite
    ├── long trail
    └── Particle Point Light Output
        bounded selected subset
        shadows off
```

This gives large-scale scene illumination from the burst while allowing selected stars to produce
local moving light without turning every spark into an expensive engine object.

# 15. Smoke — first use particles, then integrate fluids

A realistic show accumulates a lot of smoke.

Aestra already contains the `aestra-fluid` extension with significant capabilities, including gas simulation, pressure solve, advection, turbulence/combustion features and volumetric presentation.

That is more advanced than necessary for the first firework milestone.

## 15.1 Phase A — particle smoke

Start with conventional particles:

```text
Launch smoke
Burst smoke
```

using:

- textured/flipbook sprites;
- expansion over life;
- slow buoyancy;
- turbulence;
- opacity curve;
- wind influence;
- randomized lifetime/size;
- alpha or appropriate OIT strategy later.

This is enough to validate the full show.

## 15.2 Phase B — fluid smoke coupling

The fluid extension already supports concepts such as density sources and particle spawning **from** a fluid domain.

The especially valuable missing direction for fireworks is:

```text
Particle / Event
      ↓
inject density + temperature + velocity
      ↓
Fluid Domain
      ↓
Persistent volumetric smoke
```

In other words: particle → domain injection driven by actual particle positions/events.

### Desired generic coupling

```text
Inject Into Domain
├─ domain resource
├─ position source
├─ radius
├─ density
├─ temperature
├─ velocity contribution
└─ rate / event quantity
```

It should not be named "firework smoke injection".

### Acceptance criteria

- rocket can seed a narrow smoke column;
- burst injects a local smoke cloud;
- smoke remains after stars fade;
- wind moves old smoke;
- later flashes can visibly light the accumulated smoke.

---

# 16. Transparency and smoke ordering

Additive star rendering is forgiving because draw order is largely irrelevant.

Dense alpha smoke is not.

If the current renderer only sorts draw items/effects rather than every transparent particle, overlapping smoke may expose sorting artifacts.

Do not prematurely implement expensive universal GPU particle sorting.

Evaluate these options with the actual smoke prototype:

1. coarse emitter sorting only;
2. per-particle GPU depth sorting for selected renderers;
3. weighted blended OIT;
4. move high-quality smoke to volumetric fluid rendering.

Recommendation: **defer until the first particle-smoke prototype demonstrates an actual problem**.

---

# 17. Firework reference effect architecture

The showcase should be built from reusable components.

Suggested project structure:

```text
assets/effects/fireworks/
├── components/
│   ├── rocket_launch.aestra.ron
│   ├── rocket_smoke.aestra.ron
│   ├── burst_flash.aestra.ron
│   ├── gold_star.aestra.ron
│   ├── color_star.aestra.ron
│   ├── gold_crackle.aestra.ron
│   └── burst_smoke.aestra.ron
│
├── shells/
│   ├── peony.aestra.ron
│   ├── chrysanthemum.aestra.ron
│   ├── pistil.aestra.ron
│   ├── willow.aestra.ron
│   ├── crossette.aestra.ron
│   ├── crackle.aestra.ron
│   ├── palm.aestra.ron
│   └── ring.aestra.ron
│
└── shows/
    ├── fireworks_validation_show.aestra.ron
    └── fireworks_finale_stress.aestra.ron
```

The exact paths can follow the repository's final asset conventions; the important point is the compositional hierarchy.

---

# 18. Reference shell designs

## 18.1 Peony

### Purpose

First complete shell and baseline test for event burst scale.

### Graph

```text
Peony
├─ Rocket
│  ├─ Point spawn
│  ├─ narrow upward velocity
│  ├─ gravity + drag
│  ├─ Sprite
│  └─ Trail
│
├─ Rocket OnDeath → Burst Flash
└─ Rocket OnDeath → Main Stars
   ├─ spherical direction distribution
   ├─ randomized speed/lifetime
   ├─ gravity + drag
   ├─ color/opacity curve
   └─ Sprite
```

### Acceptance criteria

- visually spherical burst;
- several hundred stars supported cleanly;
- reproducible fixed seed;
- random seed gives visibly different but equivalent burst;
- scrubbing backward/forward is stable;
- no authoring workaround involving many duplicate event links.

---

## 18.2 Chrysanthemum

### Purpose

Primary trail stress test.

```text
Peony architecture
+
Main Stars → Trail Renderer
```

### Acceptance criteria

- long bright curved trajectories;
- trail remains briefly after star death where appropriate;
- no obvious point-spacing artifacts;
- adaptive/distance sampling remains stable as stars slow;
- replay does not visibly change trail shape.

---

## 18.3 Pistil

### Purpose

Validate multiple simultaneous child emitters and renderer/material diversity.

```text
Rocket OnDeath
 ├─ Outer chrysanthemum
 ├─ Inner contrasting stars
 └─ Burst flash
```

### Acceptance criteria

- two independently parameterized shells share the same event origin;
- deterministic nested timing;
- different materials/colors and velocity ranges.

---

## 18.4 Willow

### Purpose

Long-lived stateful/trail workload.

Characteristics:

- slower stars;
- pronounced gravity;
- long gold trails;
- late downward fall;
- slow fade.

### Acceptance criteria

- visually drooping strands;
- smooth behavior through very low velocities;
- dense old trails do not dominate GPU cost unexpectedly.

---

## 18.5 Crossette

### Purpose

Validate chained particle events.

```text
Rocket
 ↓
Primary stars
 ↓ OnDeath
4 child branches per star
```

### Acceptance criteria

- second-stage spawn originates exactly at parent death position;
- velocity inheritance is controllable;
- child count is authored visibly in UI;
- event buffer overflow diagnostics are testable.

---

## 18.6 Crackle

### Purpose

Stress many delayed small events.

```text
Main stars
 ↓
secondary burning carrier
 ↓
crackle spark burst
```

### Acceptance criteria

- dense small burst does not make event processing pathological;
- quality tiers can reduce spark count;
- deterministic seeds remain stable.

---

## 18.7 Palm

### Purpose

Validate richer velocity distributions.

Characteristics:

- few thick branches;
- strong coherent direction groups;
- trailing secondary sparks.

This shell should not be implemented with hardcoded firework logic. It is the acceptance case for the generic distribution system.

---

## 18.8 Ring

### Purpose

Hard acceptance test for planar directional distribution.

```text
position = single burst point
velocity = ring in oriented plane
```

Acceptance criteria:

- ring can rotate freely in 3D;
- no fake placement of particles around a pre-existing ring shape;
- ring stays coherent under speed variation and gravity;
- double ring can be built from two instances/emitters.

---

# 19. Show-level composition

The show should be authored primarily from `EffectClip` instances.

Current `EffectClip` already supports:

- source effect;
- start time;
- source offset;
- duration;
- transform;
- seed;
- parameter overrides;
- binding forwarding.

That is an excellent basis for choreography.

Example:

```text
0s                                                    30s
│──────────────────────────────────────────────────────│

Launch site A
    peony      chrysanthemum      willow

Launch site B
         peony        ring

Launch site C
                    pistil

Finale
                                      ███████████████
```

## Important show-level parameters

Shell effects should expose parameters such as:

```text
launch velocity / apex
burst size
star count
primary color
secondary color
trail lifetime
trail width
burn lifetime
burst delay
smoke amount
seed
```

Then one shell asset can produce many variants through `EffectClip.parameter_overrides`.

Avoid duplicating complete effects for every color/scale combination.

## Host-facing cue contract for sound and show control

Sound design, skybox and choreography belong to the example host, but the host must receive **actual simulation cues** to synchronize launch, burst, crackle and finale sound. A timeline estimate or pre-authored timer is insufficient when particle death/collision time varies with seeded simulation, motion or seeks.

Aestra therefore needs a bounded, documented event-output API (or an equivalent generic host binding) that exposes, for each selected cue, the effect/clip instance, emitter/link identity, trigger kind, simulation tick/time, world position, velocity, stable event identity and seed context. Specify whether cues are delivered live, suppressed or reconstructed during seek/replay, and how duplication is avoided after checkpoint restore. The Bevy example can map these cues to its own audio assets and spatial mixer; Aestra should not hardcode firework sounds.

Particle `EventLink` targets remain effect-local. If the show needs one reusable shell asset to trigger another, define an explicit host cue/clip-spawn bridge or composition feature rather than assuming cross-asset links already work. Test multiple clip instances and same-tick cues for deterministic identity and bounded delivery.

---

# 20. Variation model

A realistic show needs controlled imperfection.

Identical mathematical shells look synthetic.

Useful randomization:

```text
star speed          ±5–15%
star lifetime       ±5–20%
burst axis          slight random rotation
trail brightness    slight variation
star size           slight variation
ignition timing     milliseconds of variation
smoke quantity      variation
```

But randomness must remain deterministic for a fixed seed.

## Requirement

A repeated shell asset should support:

```text
Seed = Fixed(42)       → identical every playback
Seed = Fixed(43)       → different deterministic shell
Seed = Inherit         → stable from show instance hierarchy
```

This is already consistent with Aestra's existing seeded `EffectClip` architecture.

---

# 21. Performance target and stress tests

Do not optimize against a single shell.

The relevant target is a finale.

## 21.1 Validation workloads

### Test A — single hero shell

```text
1 shell
300–800 stars
300–800 trails
particle smoke
```

Purpose: first **compile-and-execute scale gate**, then image quality and correctness. Do not split the star/trail workload into many emitters merely to bypass a limit. Capture requested versus actual stars/trails and active versus retired histories.

### Test B — volley

```text
8–12 simultaneous shells
thousands of stars
thousands of trails
```

Purpose: normal show peak.

Gate: run after Test A passes; include simultaneous event bursts, overlapping retired tails and checkpoint seek.

### Test C — finale

Target something intentionally aggressive, e.g.:

```text
20–40 overlapping shells
20k–100k visible particles depending on renderer strategy
thousands/tens of thousands of active trail histories
heavy event generation
persistent smoke
```

Exact final budget must come from benchmark data, not from these placeholder numbers.

Gate: the counts above are workload targets, **not claims of current support**. Record the device, resolution, frame-time target, VRAM budget and quality tier for every result. If the high tier misses its budget, identify the measured pass and implement an explicit tier reduction; do not hide loss behind queue or buffer truncation.

## 21.2 Profile separately

Measure:

- simulation dispatch time;
- stateful update time;
- event collection/expansion;
- event ordering/compaction and requested/emitted/dropped counts at each boundary;
- trail history update;
- trail compaction;
- trail culling;
- trail geometry generation;
- active/retired trail owners, eviction/truncation, scratch and history memory;
- sprite rendering;
- material evaluation;
- post-processing/bloom;
- smoke;
- checkpoint memory;
- seek/replay latency;
- nested `EffectClip` overhead.

## 21.3 LOD / quality tier behavior

Fireworks are ideal for scalable quality.

Potential knobs:

```text
star count
trail sample rate
trail max points
trail lifetime
secondary crackle count
smoke count/resolution
fluid grid resolution
bloom quality
lighting enable/disable
```

The authored effect should remain the same logical asset while the compiler/runtime selects a lower quality plan.

Firework star counts, event fan-out and trail budgets are **not yet proven to scale by tier** in the current implementation; this is a requirement, not a description of existing behavior.

Record the chosen effective capacities/counts with the compiled plan so a tier switch and deterministic replay use the same plan. Define a minimum visual contract for each tier (for example, no missing primary trails at high tier); optional detail may degrade, primary authored structure may not silently vanish.

---

# 22. Proposed implementation roadmap

## Milestone F0 — Baseline + dedicated validation scene

**Goal:** establish a repeatable visual/performance environment before changing architecture.

**Implementation status (2026-09-29): validation fixture and baseline delivered; exit gate partial.** The runnable deliverable is currently the viewer's `--fireworks-f0` fixture, not a separate `fireworks_validation_show` asset. [F0 run instructions and measured results](../../benchmarks/fireworks/README.md) and the [machine-readable baseline](../../benchmarks/fireworks/baseline-2026-09-29.json) are checked in with the implementation.

### Tasks

- Add a dark outdoor preview/reference scene.
- Add ground plane and simple geometry for light-response testing.
- Add fixed cameras:
  - close shell camera;
  - audience camera;
  - wide finale camera.
- Define GPU profiling capture procedure.
- Add isolated event and trail microbenchmarks at and beyond the current 64/256/1,024 limits, including coincident deaths and retained tails.
- Record current compile failures, truncation behavior, GPU time, memory and seek cost as baseline evidence; do not label a rejected workload as a performance result.
- Define deterministic screenshot/reference seeds.
- Create an initial crude radial firework with current features.

### Deliverable

`fireworks_validation_show` can be played and benchmarked even though it is visually incomplete.

### Exit criteria

- deterministic playback;
- stable screenshot positions;
- baseline profiler numbers committed/documented.
- target device/quality-tier budgets documented for Tests A–C, with an inventory of available and missing requested-versus-produced counters.

### Implemented and measured

- Added a fixed-ID, fixed-seed, three-emitter radial sketch: rockets produce 48 stars on death; star collisions produce glints. The viewer supplies a dark 3D scene, ground and geometry markers, and fixed close, audience and wide cameras. Its exact-frame capture and GPU-timestamp benchmark can run the same fixture without an editor session.
- Added isolated event fan-out and trail-owner probes, plus compiler-boundary tests. At F0, one link compiled at count 64 but rejected 65 and 800; F1 now accepts 800 under the list-memory budget. F0 rejected trail emitters at 257 and 800 parents; F1B's first slice now accepts both, through 1,024 parents. Historically rejected cases have **no F0 runtime timing claim**.
- The supported event probe requests 4,096 children from 64 coincident source deaths; the frame-60 GPU report shows 1,024 live children in a 4,096-slot destination and no warning. This is evidence of silent loss, but the missing stage counters prevent exact attribution.
- The supported trail probe reaches 256 occupied trails. At frame 120 it reports 183 truncated trails; after parent deaths, frame 300 reports 82 retired trails, 175 truncated trails and zero evictions. The later tails are outside the close camera, so visual tail quality is not established.
- Recorded three baseline runs on an RTX 4070 SUPER/Vulkan at 960 × 540, high tier, with 120 warm-up and 600 measured frames per run. Baseline simulation p95 spans 0.454–0.554 ms. The final 256-owner trail probe repeats at 8.557 and 8.589 ms simulation p95, before a production-density hero workload can even compile. Corrected the benchmark report so it retains the configured warm-up count.
- Documented provisional high-tier Test A–C time, buffer and seek targets, and which requested-versus-produced measurements are available or missing. These are goals, **not** verified performance results or lower-tier budgets.

### Remaining F0 exit work

- The original exact-frame audience captures matched through frame 120 but differed at frames 180 and 300. F1 isolated transparent presentation order, added a lockstep GPU replay regression and obtained matching fresh-launch PNG hashes at frames 120, 180 and 300 with opt-in stable capture. Normal game playback stays on the unsorted fast path; higher-density exact visual ordering is a capture-only concern, not a runtime-playback gate.
- Complete event requested/captured/expanded/spawned/dropped counters and overflow diagnostics before treating fan-out timing as valid produced-work performance. F1's first slice now counts and warns about dropped children at each link, but the other stages and profiler exposure remain open. F1 owns the scalable event contract; F1B owns production-density trails and truncation policy.
- Measure isolated cold-seek latency and actual effect GPU allocation. Recalibrate the provisional budgets at 1920 × 1080 and on lower quality tiers. The current 960 × 540 baseline does not establish release performance.

---

## Milestone F1 — Scalable event pipeline and event-link authoring

**Goal:** make one logical event link reliably produce a hero burst and keep heavy event chains deterministic.

**Status: in progress.** The first runtime slice exposed the original 3,072-child loss in each 4,096-child probe cohort, then sized each link list for captured source capacity × fan-out; the frame-60 GPU capture produced all 4,096 stars. The next slice replaced the quadratic ordinal-rank scan with a deterministic workgroup sort and indirect parallel child expansion. The authored single-link fan-out limit is now 800, with checked per-effect list planning and a conservative 128 MiB aggregate list budget; a new one-rocket/800-child probe measures all 800 stars live on the GPU. GPU counters distinguish captured child demand, list omissions and destination accepts, with warnings for rejected destination work. Conformance covers reversed source order, 1,024 coincident source events, 800 children from one event, an undersized list, and destination slot exhaustion. A production lockstep regression proves bit-exact per-ordinal particle records, captured events, spawn counts and transparent draw ordinals for a chained firework across fresh GPU runs and a backward checkpoint seek. An **opt-in** GPU capture pass sorts up to 4,096 live particles per emitter; fresh audience PNG hashes match at frames 120, 180 and 300 with `--stable-transparency`. The portable sort raised the 4,096-child event probe's aggregate simulation p50/p95/p99 from 0.292/0.325/0.497 to 1.499/1.543/1.758 ms on the named RTX 4070 SUPER/Vulkan setup, so normal live playback skips it by default and measured 0.344/0.376/0.601 ms in a new run. Above 4,096 live particles per emitter, exact capture draw order remains uncanonicalized; **this does not cap live playback**. The **source-event capture buffer remains capped at 1,024**, and warning counters are not yet in the profiler or capture report. Device/tier-aware budgets, source overflow policy, concurrent shells, and editor feedback for the new budget remain F1 runtime exit work. Scalable exact-image capture is a separate optional target, not a prerequisite for real-time game playback.

**Runtime priority:** certify the default fast path under sustained hero, volley and finale concurrency: produced-versus-requested counts, explicit overflow behavior, GPU simulation and rendering p95/p99, and memory on the target tiers. Checkpoint replay remains a useful editor/testing regression, but exact seeking and bit-identical transparent pixels are not prerequisites for game playback. Do not spend the live-frame budget on capture-only ordering.

### Core/editor tasks

- Remove stale unsupported-runtime label.
- Add editable event-link selection/inspector.
- Add `count` editing.
- Add `inherit_velocity` editing.
- Add proper semantic commands and undo/redo.
- Add validation messages in inspector.
- Save/reload tests.

### Runtime/core tasks

- Implement the capacity planning, deterministic parallel event ordering/expansion, chunking and overflow contract in section 6.
- Replace the remaining fixed 1,024-entry **source** capture assumption; account for destination capacity and fan-out across links under adapter- and tier-aware budgets. The child-list assumption has been removed, and destination rejection is counted, but source overflow still loses event identities.
- The authored `MAX_EVENT_LINK_COUNT` is now 800; keep that limit separate from device/tier budgets and expose the aggregate-list budget in editor feedback before treating large fan-outs as production-ready.
- Surface requested, emitted and dropped counters prominently in profiling/diagnostics.

### Tests

- authoring command tests;
- serialization migration/contract tests;
- compiler tests;
- GPU conformance with high child counts;
- deterministic event ordering test;
- same-tick many-event and chained-fan-out tests;
- source, expansion and destination overflow-policy tests;
- checkpoint/replay and measured GPU-cost tests (chained-event exact-state replay and opt-in baseline-density visual ordering are covered; concurrent-shell live cost remains open; scalable exact-image capture is optional).

### Deliverable

Rocket can author and *execute* a 300–800-star burst through one logical event link with no silent drops; section 6's acceptance gate passes.

---

## Milestone F1B — Scalable trail histories

**Goal:** remove the trail implementation ceilings before building a production Chrysanthemum or Willow.

The pre-F1B default-fast 256-owner probe measured 8.820/9.160/182.990 ms simulation p50/p95/p99. Its history shader handled each emitter on one invocation with repeated parent × owner scans and serial bounds; trails also paid for serial ribbon sorting. The first slice replaces those paths and adds isolated particle/history/compaction timestamps. Live playback remains the priority, not checkpoint optimization.

### Implemented — cooperative trail runtime

- One 64-lane workgroup per emitter builds a stable-ID-to-owner hash from retained history. Surviving parents reserve their owners before any births allocate; empty owners precede oldest retired tails, with deterministic chunk-index tie breaks. A parallel birth scan assigns disjoint owners without serial searches.
- Parent ordering, history sampling/expiry, world-space bounds and occupied/retired/truncated statistics are parallel. Time, distance and adaptive sampling retain their existing semantics. Trail-only emitters no longer run ribbon linking; mixed Trail/Ribbon emitters link after cooperative ordering.
- Owner-prefix compaction is a parallel exclusive scan. Candidate/draw ordering and alpha semantics remain unchanged.
- Compiler/GPU validation now accepts up to 1,024 parents in one trail emitter. The shared algorithm uses the portable 16 KiB workgroup budget and the existing eight storage bindings. Particle, aux and checkpoint layouts are unchanged; the lookup is rebuilt on restore.
- Real GPU tests cover 800 heads in two independent emitter workgroups, colliding/high stable IDs, shuffled live-slot order, survivor history retention, deterministic oldest-tail eviction, full 1,024-parent pools, pause, expiry and mixed Ribbon linking. Existing sampling, reset/seek/checkpoint, UV, bounds and culling tests pass; compaction covers 70, 800 and 1,024 owners. Portable WGSL/SPIR-V/HLSL validation passes.
- The viewer retains the original `trail` probe for before/after comparison and adds `--fireworks-f0-probe trail-hero`: one analytic 800-parent burst, 1,024 owners, 32 records/owner. Its settings keep trails within the close camera; it is a workload probe, not a finished fireworks shell.
- A frame-120 native-GPU capture of that hero probe measures **800 occupied histories, zero evictions and zero truncated histories**, with all 800 parents supported in one emitter. The separate two-emitter conformance test covers retired-tail retention and pool exhaustion.

Measured on RTX 4070 SUPER/Vulkan, 960 × 540, high tier, default-fast transparency, 120 warm-up and 600 measured frames:

| 256-parent probe GPU metric | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| Full simulation window | 0.350 ms | 6.641 ms | 11.409 ms |
| Isolated particle observation | 0.030 ms | 0.052 ms | 0.091 ms |
| Isolated trail-history observation | 0.271 ms | 0.514 ms | 0.941 ms |
| Trail compaction | 0.094 ms | 0.173 ms | 0.276 ms |

The full simulation median is approximately 25× lower than the pre-F1B default-fast probe. **This is not full-frame budget certification:** aggregate high-percentile spikes remain. Individual observation diagnostics are the last observation published under that path, not the sum of every observation in a reconstruction frame. Do not attribute the difference to one stage without a trace or sum per-instance/per-observation timings as though they were the whole frame.

The 800-parent/1,024-owner hero probe records full simulation **0.368/9.342/12.971 ms**, isolated history observation **0.307/1.042/1.532 ms**, compaction **0.045/0.092/0.111 ms**, and transparent drawing **0.015/0.048/0.061 ms** (p50/p95/p99). These whole-run distributions include birth/retirement/empty phases, not a sustained maximum-occupancy finale, and are subject to the same aggregate-spike caveat.

Raw reports and captures are reproducible with:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail --camera close --backend gpu --gpu-bench target/fireworks-f1/trail-parallel-256.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail-hero --camera close --backend gpu --gpu-bench target/fireworks-f1/trail-parallel-800.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail-hero --camera close --backend gpu --sample-frames 0,30,120,239,250,270,300 --capture target/fireworks-f1/trail-parallel-800-capture
```

### Implemented — event-born stateful trail integration

- Fully stateful effects previously skipped trail recording entirely; mixed effects recorded before their stateful presentation. Effects combining histories and stateful simulation now use the production lockstep encoder. Each fixed tick integrates particles, expands event births, presents analytic and stateful heads at the same canonical time, then records histories. Ordinary playback advances only new ticks; there is no analytic reconstruction of event-born heads or CPU particle readback.
- Catch-up observes every processed tick, including short-lived particles born between rendered frames. Presentation and trail expiry use the processed tick, not an eventual seek target. Pausing does not append samples. Histories intentionally retain canonical 60 Hz observations rather than contaminating them with render-frame-dependent sub-tick samples.
- Backward seeking restores only a checkpoint common to particles, coupled domains and histories. Without a joint checkpoint, all stores restart together under the existing bounded catch-up budget. Changed input histories discard invalid future history snapshots. History snapshots share the existing global 64 MiB trail-checkpoint budget; larger/faster checkpoint storage is not this slice's objective.
- A native GPU regression drives the production event gather/spawn and lockstep encoder: one rocket death births 800 stars, each with a non-degenerate history; all tails retire and expire. It checks pure-stateful and mixed analytic/stateful effects, pause, bounded catch-up, restart, checkpoint restore with a changed discontinuity epoch, and bit-identical ordered histories compared with uninterrupted playback.
- The viewer adds `--fireworks-f0-probe event-trail`, a single-link 800-star stateful target with sprite heads and 1,024 trail owners. Its first-cohort frame-50 capture on RTX 4070 SUPER/Vulkan measures **800 occupied histories, zero evictions and zero truncation**. It is a technical workload, not a production shell. The recurring capacity-one rocket intentionally requests later cohorts: destination-slot rejections and eventual retired-tail eviction are reported, not hidden or interpreted as successful full-demand playback.
- Per-instance GPU simulation timestamps now include the fully stateful/history lockstep path. A paused capture is not a sustained live-performance benchmark; the previous analytic timings above do not certify this new event-to-trail workload.

Reproduce the first-cohort capture with:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail --camera close --backend gpu --sample-frames 50 --capture target/fireworks-f1/event-born-trails-first-cohort
```

The live event-to-trail benchmark (RTX 4070 SUPER/Vulkan, 960×540, 120 warm-up and 600 measured frames) records GPU simulation **3.237/3.908/3.959 ms p50/p95/p99**, trail compaction **0.212/0.252/0.258 ms**, and transparent drawing **0.128/0.218/0.250 ms**. CPU simulation encoding is **0.094/0.121/0.147 ms**. These are separate measured stages, not a sum of frame percentiles. The recurring workload runs under the finite destination/history pressure described above, so these measurements do **not** certify that every authored later-cohort request was produced, or that the hero/finale budget is met. They are not directly comparable to the analytic-only probe.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail --camera close --backend gpu --gpu-bench target/fireworks-f1/event-born-trails-bench.json
```

### Implemented — paged trail draw compaction

- Replaced the draw-compaction prefix's single-workgroup logical-owner ceiling with 1,024-owner pages. Each page scans in its own workgroup; a separate scan prefixes page totals, and scatter combines page/local offsets in exact original owner/primitive order. The page-total scan itself processes tiles, rather than introducing another 1,024-page ceiling.
- Workgroup scratch remains bounded to 4 KiB plus one word. Temporary storage adds one word per page, with no new binding, persistent history layout change, checkpoint work or CPU readback in the live path. Pools through 1,024 owners retain the three-dispatch path and skip page-offset loads; larger pools use four dispatches.
- Native GPU tests exercise sparse page boundaries, partial last pages, dense 8,192-owner histories, all UV/cap combinations, retirement/expiry, repeated frames and cleared restart state. A separate prefix-only test checks exact offsets for 1,048,583 synthetic owner counts across 1,025 pages, plus empty/singleton cases. It does not allocate those histories or certify million-owner runtime support.
- The existing event-to-trail live fixture (same RTX 4070 SUPER/Vulkan, 960×540, 120 warm-up/600 measured frames) measures compaction **0.215/0.257/0.259 ms p50/p95/p99**, compared with the previous **0.212/0.252/0.258 ms**; simulation is **3.234/3.914/3.959 ms**. This checks the small-pool production path, not large-pool performance. Later-cohort destination/history pressure still applies; no full-demand/finale budget certification is inferred.
- This slice supplied drawing infrastructure only; history allocation still had the 1,024-parent/owner limit at that point. The following slice removes that limit. Overlapping-shell/finale certification remains open.

```powershell
cargo test --locked -p aestra-bevy-render --test trail_compaction_conformance -- --nocapture
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail --camera close --backend gpu --gpu-bench target/fireworks-f1/paged-compaction-final-bench.json
```

### Implemented — paged history allocation and resource preflight

- Large pools sort live identities and occupied owners using 1,024-element workgroup pages plus storage-backed parallel stable merges. Binary lookup reserves surviving owners before birth allocation; page-prefix birth ranks select empty owners first, then oldest retired tails with physical-owner tie breaks. Sampling is shared with the small-pool kernel, and page/global reductions produce bounds and usage counters. No CPU history readback or per-tick clear occurs during live playback.
- Temporary aux ranges are disjoint and checked. Initially, for power-of-two padded head/owner counts `H`/`O`, scratch added `3H + 2O + ceil(H/1024) + 12ceil(O/1024)` words per large emitter; the finer bounds pages implemented below update the last term to `12ceil(O/64)`. Spare emitter/globals lanes carry exact numeric offsets and transient stage parameters, so particle records, owner headers, sampling and persistent aux layouts are unchanged. Existing checkpoints also copy the appended scratch; scratch is reconstructed on the next observation rather than treated as authoritative ownership state. Excluding disposable scratch from checkpoint copies is deferred editor optimization.
- GPU preparation checks record count, arithmetic overflow, scratch representation, adapter storage-binding/buffer size and dispatch dimensions before capacity-sized artifact allocation. Unsupported resources report the backend rejection instead of overflowing. The one-Trail-renderer and 2–64-point rules remain; compiler, model and editor no longer clamp parent/owner counts to 1,024. Static memory estimates include the new scratch and compaction page totals.
- Native GPU conformance covers 1,025/2,053 partial pools and 8,192 dense owners, high/colliding identities, shuffled head order, independent emitters, survivor retention, deterministic eviction, pause, backwards-clock reset, retirement/expiry and mixed Ribbon linking. Production fixed-tick encoding covers 2,048/8,192 event-born stars with doubled owner budgets, mixed analytic presentation, pause and batched restart. Existing 800-star checkpoint/playback tests and portable WGSL/SPIR-V/HLSL validation still pass under eight storage bindings and a 16 KiB workgroup limit.
- The viewer adds `--fireworks-f0-probe event-trail-large`: sixteen coincident source deaths × 512 children into **one 8,192-star emitter**, 16,384 history owners and 32 records/owner. This uses one supported event link, not duplicate links or trail emitters to bypass a limit. Its frame-50 RTX 4070 SUPER/Vulkan capture measures **8,192 occupied histories, zero retired tails, zero evictions and zero truncation**. The image is a technical density probe, not a finished realistic shell.
- Same adapter, 960×540, high tier, default-fast transparency, 120 warm-up/600 measured frames: full GPU simulation **5.263/6.431/7.261 ms**, trail compaction **0.224/0.250/0.307 ms**, transparent draw **0.496/0.874/0.920 ms** (p50/p95/p99). Simulation CPU encoding is **0.182/0.248/0.337 ms**. These are aggregate recurring-capacity-pressure distributions, not isolated allocation costs or named full-frame budget certification. Later cohorts intentionally request more than the 8,192 live destination slots; destination rejections are warned/counted and must not be mistaken for successful full-demand playback.

```powershell
cargo test --locked -p aestra-gpu --lib --test trail_contract --test shader_contract
cargo test --locked -p aestra-bevy-render --test trail_conformance --test trail_compaction_conformance -- --nocapture
cargo test --locked -p aestra-bevy-render --lib event_born -- --nocapture
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-large --camera close --backend gpu --sample-frames 50 --capture target/fireworks-f1/paged-history-first-cohort
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-large --camera close --backend gpu --gpu-bench target/fireworks-f1/paged-history-large-bench.json
```

### Implemented — budgeted live volley and phase-level cost attribution

- Added `--fireworks-f0-probe event-trail-volley`: four genuine source deaths per second, each requesting 800 stars through one link into **one 8,192-slot destination and one 16,384-owner trail pool**. Stars live for two seconds; trails retain one second of history at 30 Hz with 32 points. Budgets include retired tails and fixed-tick boundary overlap, rather than accepting a faster run caused by destination loss. This is a technical repeated-cohort baseline, not eight distinct authored shell assets, a smoke workload, or a finished Test B show.
- Native production-lockstep regression runs 16 cohorts and then drains. Every tick checks captured child demand versus destination acceptance, actual source capture overflow, expansion omissions, live particles versus live owners, evictions and truncation. **All 12,800 requested children are accepted**; peaks are **7,200 live stars, 3,200 retired owners and 10,400 occupied owners**. No source overflow, list omissions, destination rejection, eviction or truncation occurs, and all histories expire after emission stops.
- `GpuEventLinkStatistics` now exposes observed captured demand, expansion omissions, accepted children and destination rejections per compiled link, plus source-event overflow. These are asynchronous buffer-lifetime activity totals, including warm-up; replay/reset activity can be counted again. They are not current-epoch authoritative counts. No particle readback or new GPU counter buffer is required. The benchmark JSON records these totals alongside measured live/history peaks and estimated buffer memory; absent observations remain `null`, not a claimed zero.
- Stateful history recording now has an isolated GPU timestamp, with paged sub-phases for head ordering/presentation, owner ordering/reservation, birth allocation/sampling, and bounds. Each phase includes its complete sort/merge sequence; a repeated merge label does not hide earlier merges. Diagnostics still publish the last observation for a path in a multi-tick frame, not the sum of reconstruction work. Do not sum phase percentile values into a whole-frame result.
- RTX 4070 SUPER/Vulkan, **960×540, high tier, close camera, default-fast transparency, 120 warm-up/600 measured frames**: the phase-instrumented run observes **37,600 captured children and 37,600 accepted**, zero loss at every exposed boundary, **7,201 live particles** (including the rocket), **10,400 occupied/3,200 retired histories**, and **38,438,072 bytes estimated buffer memory** (not total VRAM or checkpoint memory). Async snapshots can lag; the every-tick native regression independently verifies admission and drain.
- The deterministic frame-240 (4.0 s) capture renders the overlapping trails and reports **9,600 occupied/3,200 retired owners**, zero evictions and zero truncation. It is an overlap inspection, not the peak-count or realistic-image-quality gate.
- GPU p50/p95/p99 in milliseconds: full simulation **5.273/5.642/6.423**, trail history **5.042/5.361/5.431**, trail compaction **0.251/0.274/0.402**, transparent draw **0.873/0.962/0.987**. Simulation CPU encoding is **0.096/0.155/0.244 ms**. History-phase medians are heads **1.032 ms**, reservation **1.541 ms**, allocation/sampling **1.612 ms**, bounds **0.828 ms**. History remains the dominant live cost; complete frame/post-process/smoke budgets and finale certification are not inferred from these pass timings. The pre-phase-instrumentation run measured simulation **5.118/5.464/6.196 ms**, so these figures also include diagnostic overhead and run variation.

```powershell
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --lib budgeted_trail_volley -- --nocapture
cargo test --locked -p aestra-viewer overlapping_volley -- --nocapture
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/budgeted-volley-phases-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --sample-frames 240 --capture target/fireworks-f1/budgeted-volley-overlap
```

### Implemented — cached-key page sorting for live histories

- Sorting pages shrink from 1,024 to **256 entries** to expose more workgroups. Each page caches immutable sort keys in shared memory once, instead of repeatedly loading strided particle records during sorting-network comparisons. Head keys retain strand/stable-ID/physical-index order; occupied owners retain stable-ID/physical-index order; allocation candidates retain empty-first, then oldest-retired, then physical-index order. Survivor reservation and deterministic allocation are unchanged.
- Sorting now uses **8 KiB shared storage** (the existing 1,024-word scan array plus 256 four-word keys). The small-pool path still fits the portable 16 KiB limit, and eight storage bindings are retained. Prefix and bounds pages remain 1,024 entries; persistent records, aux/scratch ranges, physical capacities and estimated buffer memory are unchanged. Smaller sorting pages add merge passes; the dispatch plan and parity change together, guarded by tests including a 16-head/2,048-owner pool.
- Native tests include stable IDs `0` and `u32::MAX`, shuffled presentation, partial/dense pools, mixed Ribbon ordering, survivor retention, deterministic retired eviction, pause/restart, retirement/expiry, and the every-tick loss-free volley/drain regression. The maximum stable identity is not confused with the missing **list index** sentinel. Portable WGSL/SPIR-V/HLSL contracts and refreshed generated snapshots pass.
- The fixed frame-240 overlap capture produces a **byte-identical PNG** to the committed baseline (same SHA-256), with matching 9,600 occupied/3,200 retired owners and zero evictions/truncation. This checks that frame's output, not complete visual equivalence for every asset or playback time.
- Same RTX 4070 SUPER/Vulkan, 960×540, high tier, close camera, default-fast transparency, 120 warm-up/600 measured frames. Both optimized runs and a fresh committed-sort control observe **37,600 captured children and 37,600 accepted**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, zero source overflow, expansion omissions, destination rejection, eviction or truncation, and **38,438,072 estimated buffer bytes**. Timing improvements therefore do not come from smaller populations or silently dropped work.

GPU phase medians in milliseconds (control was rerun after the optimized samples to check current run variation):

| Phase | Committed-sort control | Cached sort, run 1 | Cached sort, run 2 |
| --- | ---: | ---: | ---: |
| Head ordering/presentation | 0.905 | 0.422 | 0.436 |
| Owner ordering/reservation | 1.208 | 0.555 | 0.561 |
| Allocation/sampling | 1.316 | 0.652 | 0.666 |
| Bounds | 0.755 | 1.243 | 1.115 |
| History observation | 4.095 | 2.937 | 2.849 |
| Full simulation frame | 4.390 | 3.290 | 3.315 |

- History median is **28–30% lower than the fresh control** (42–44% below the earlier 5.042 ms phase-instrumented baseline). However, bounds and transparent drawing were slower in the optimized samples; a history speedup is not the same as a full-frame speedup. Whole-simulation p95/p99 vary: optimized run 1 **3.840/5.800 ms**, run 2 **6.027/6.972 ms**, control **6.406/9.858 ms**. History p95/p99 are optimized **3.384/3.540** and **3.412/3.512 ms**, control **5.182/5.626 ms**. These last-observation diagnostic paths and aggregate simulation frames must not be conflated, particularly when live frames process multiple fixed ticks. No full-frame/finale percentile certification is inferred.

```powershell
cargo test --locked -p aestra-gpu --lib --test trail_contract --test shader_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --nocapture
cargo test --locked -p aestra-bevy-render --lib trail -- --nocapture
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/cached-sort-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --sample-frames 240 --capture target/fireworks-f1/cached-sort-volley-overlap
```

### Implemented — finer parallel bounds reduction for live histories

- Bounds pages shrink from **1,024 to 64 owners**: each of the 64 lanes scans one owner's ring rather than serializing sixteen owners per lane. A 16,384-owner pool now dispatches **256 page workgroups instead of 16**, followed by the existing GPU summary reduction. Owner expiry, sample order, survivor-flag consumption, occupied/retired/truncated counts, and finite/empty/unsafe bounds semantics are unchanged. No new pass, storage binding, CPU readback or live replay work is introduced.
- At this stage, scratch is **`3H + 2O + ceil(H/1024) + 12ceil(O/64)` words** per paged emitter; the compact-key cache below subsequently appends another `2max(H,O)` words. Allocation, dispatch and static memory estimates change together. The volley estimate increases from **38,438,072 to 38,449,592 bytes**: only **11,520 extra bytes (11.25 KiB)** of disposable summaries. Tests guard shader/planner page-size agreement, disjoint ranges, binding/buffer/dispatch rejection and exact numeric offset limits. Persistent particle/history/aux ownership ABI, sorting and the small-pool path are unchanged; portable 16 KiB workgroup/eight-binding conformance still passes.
- Native tests check full and partially filled 1,025/2,053/8,192-owner pools against an all-samples world-bounds oracle, plus paged Distance/Adaptive sampling with stationary-anchor expiry, truncation warnings, empty pages, invalid bounds disabling culling, and the 64-point maximum. The real event-born volley still accepts **12,800/12,800** children, reaches **7,200 live / 3,200 retired / 10,400 occupied** histories, and drains completely without eviction or truncation.
- Two optimized benchmarks and a fresh 1,024-owner-page control use the same RTX 4070 SUPER/Vulkan, 960×540, close camera, default-fast transparency and 120 warm-up/600 measured frames. Every run observes **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, and zero source overflow, expansion omission, destination rejection, eviction or truncation. Frame 240 is again a **byte-identical PNG** to the pre-optimization capture, with 9,600 occupied/3,200 retired histories; this checks that frame, not every possible asset/time.

GPU medians in milliseconds (fresh control rerun after both optimized samples):

| Phase | 1,024-owner-page control | 64-owner pages, run 1 | 64-owner pages, run 2 |
| --- | ---: | ---: | ---: |
| Bounds | 1.214 | 0.264 | 0.261 |
| History observation | 2.903 | 2.343 | 2.320 |
| Full simulation frame | 3.255 | 2.737 | 2.703 |
| Trail compaction | 0.361 | 0.460 | 0.443 |
| Transparent drawing | 1.269 | 1.637 | 1.572 |

- Bounds median is **78% lower**, history median **19–20% lower**, and aggregate simulation median **16–17% lower** than this control. History p95/p99 changes from **3.398/3.511 ms** to **2.627/2.672** and **2.561/2.647 ms**. Aggregate simulation p95/p99 changes from **3.854/5.947 ms** to **3.087/5.213** and **3.065/5.297 ms**. The unchanged ordering/allocation and draw phases are slower in these optimized runs; the reason is not established. Do not sum phase percentiles or infer a certified full-frame/finale budget from the history speedup. Remaining ordering/reservation/allocation traffic and multi-tick frame variation still require investigation.

```powershell
cargo test --locked -p aestra-gpu --lib --test trail_contract --test shader_contract
cargo test --locked -p aestra-runtime --lib profile
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --nocapture
cargo test --locked -p aestra-bevy-render --lib trail -- --nocapture
cargo clippy --locked -p aestra-gpu -p aestra-runtime -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/parallel-bounds-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --sample-frames 240 --capture target/fireworks-f1/parallel-bounds-volley-overlap
```

### Implemented — compact keys for global merge and owner lookup

- Each ordering phase now writes **two immutable key words per physical index** into a contiguous aux cache while forming the initial pages. Global merges retain each lane's key once and load compact candidate keys instead of repeatedly fetching strided particle/history records. Survivor lookup also searches cached stable IDs. Each binary-search step uses one directional comparator, retaining left-before-right sentinel stability. The cache is rebuilt for heads, occupied owners, and empty/retired allocation candidates in order; it is neither a persistent identity map nor authoritative history state.
- Current scratch is **`3H + 2O + ceil(H/1024) + 12ceil(O/64) + 2max(H,O)` words** per paged emitter. The cache follows the padded bounds-summary range and cannot alias sort lists, birth ranks, page prefixes, or another emitter. Allocation/preflight and static memory estimates include it. The volley estimate is **38,580,664 bytes**, an increase of **131,072 bytes (128 KiB)** from the finer-bounds version. There is no new pass, binding, workgroup-memory allocation, live readback or replay work; the small-pool path and persistent ownership/sample ABI remain unchanged.
- Native conformance now keeps both stable identities **`0` and `u32::MAX`** alive through ordinary reservation, and independently predicts each birth's physical owner using empty-first/oldest-retired/physical-index order and the mixed Ribbon emitter's strand-first birth order. This oracle passes both the committed control and the optimized implementation, including partial/dense pools and offset emitter ranges. Existing bounds, spatial expiry/truncation, pause/reset, maximum-ring, eight-binding/16 KiB, portable WGSL/SPIR-V/HLSL, and every-tick volley/drain tests pass.
- Same RTX 4070 SUPER/Vulkan, 960×540, close camera, high tier, default-fast transparency, 120 warm-up/600 measured frames. Both optimized runs and a fresh committed-code control observe **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, and zero source overflow, expansion omission, destination rejection, eviction or truncation. Frame 240 remains a **byte-identical PNG** with 9,600 occupied/3,200 retired histories; this checks that frame, not all possible playback times.

GPU medians in milliseconds (committed control rerun after both optimized samples):

| Phase | Committed control | Compact keys, run 1 | Compact keys, run 2 |
| --- | ---: | ---: | ---: |
| Head ordering/presentation | 0.513 | 0.457 | 0.466 |
| Owner ordering/reservation | 0.678 | 0.584 | 0.593 |
| Allocation/sampling | 0.819 | 0.699 | 0.710 |
| Bounds | 0.257 | 0.262 | 0.270 |
| History observation | 2.311 | 2.036 | 2.072 |
| Full simulation frame | 2.713 | 2.436 | 2.477 |
| Trail compaction | 0.449 | 0.459 | 0.472 |
| Transparent drawing | 1.569 | 1.663 | 1.675 |

- History median is **10–12% lower**, and aggregate simulation median **9–10% lower**, than the fresh control. History p95/p99 changes from **2.564/2.672 ms** to **2.272/2.340** and **2.295/2.359 ms**. Aggregate simulation p95/p99 changes from **3.042/5.315 ms** to **2.725/4.923** and **2.802/4.976 ms**. Bounds/compaction/drawing are slightly slower in the optimized runs; their cause is not established. Last-observation phase paths still do not describe all ticks of a catch-up frame. Do not sum percentile timings or call the named full-frame/finale budget certified.

```powershell
cargo test --locked -p aestra-gpu --lib --test trail_contract --test shader_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --nocapture
cargo test --locked -p aestra-bevy-render --lib trail -- --nocapture
cargo clippy --locked -p aestra-gpu -p aestra-runtime -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/compact-keys-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --sample-frames 240 --capture target/fireworks-f1/compact-keys-volley-overlap
```

### Implemented — frame-aligned live simulation work attribution

- `GpuSimulationTiming::frame_sample` exposes a context-valid **timestamp-batch sequence, requested playback time, whole per-effect GPU simulation window, executed shared fixed ticks, cumulative history observations/workgroups, and actual coupled particle/trail checkpoint capture bytes**. Work metadata travels with its own timestamp result through the existing bounded asynchronous mailbox, not with an independently arriving live-count readback. No new GPU query pair, particle readback, synchronous wait, shader pass or replay work is added. Unsupported/skipped/failed timestamps remain unavailable; independent/analytic tick and checkpoint counts remain unknown. Legacy mixed analytic/stateful partial timing windows are excluded from this complete-window API. Requested time does not certify that paced catch-up reached it; checkpoint bytes exclude domain snapshots and restores.
- The viewer JSON now contains raw `simulation_frames`, `simulation_total`, and `simulation_by_work` distributions per presented effect. Retained timestamp samples are counted once by sequence; diagnostic-store measurements are likewise counted once by measurement time, with warm-up cursors consumed and non-finite values omitted. The capture remains a **host-received window**, not a GPU-frame barrier: late warm-up results can arrive within it and final submitted frames can arrive after it ends. Population peaks and lifetime event totals remain explicitly asynchronous and are not joined to these frame samples.
- History workgroups count the actual padded update/page/merge dispatch plan across **every** observation, including its emitter dimension; they are not live-head or live-owner counts and exclude ribbons. Tick zero initialization can observe histories twice; a subsequent zero-tick/paused frame still performs one history observation. Native regressions check these cases, budget-limited multi-tick accumulation, checkpoint bytes only when captures really occur, unchanged paused history, and every-tick volley acceptance/retirement/drain. Timestamp tests preserve work metadata through native GPU map/recycling, distinguish missing/partial results, isolate repeated owners, reject stale contexts, and keep the latest-frame mailbox bounded. Viewer tests guard warm-up/stale/duplicate observations and JSON grouping.
- Two completed benchmarks use the same RTX 4070 SUPER/Vulkan, 960×540, close camera, high tier, default-fast transparency and 120 warm-up/600 measured host frames. Both obtain **600 unique complete-window samples**, observe **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, zero source overflow, expansion omission, destination rejection, eviction or truncation, and the unchanged **38,580,664-byte configured-buffer estimate**. That estimate is not total resident memory including checkpoint snapshots. The frame-240 PNG is again byte-identical to the compact-key reference, with 9,600 occupied/3,200 retired histories; this checks that frame, not every asset/time.

Frame-aligned per-effect simulation timing, milliseconds; parentheses give sample counts, not a fixed per-run tick mix:

| Encoded frame work | Run 1 p50 / p95 / p99 | Run 2 p50 / p95 / p99 |
| --- | ---: | ---: |
| 0 ticks, 1 history observation, no checkpoint copies | 0.313 / 1.320 / 1.320 (6) | 1.258 / 1.367 / 1.380 (23) |
| 1 tick, 1 observation, no checkpoint copies | 2.487 / 2.732 / 2.828 (557) | 2.186 / 2.672 / 2.700 (527) |
| 1 tick, 1 observation, 32,715,088 checkpoint bytes | 4.743 / 5.050 / 5.086 (30) | 4.448 / 5.063 / 5.115 (26) |
| 2 ticks, 2 observations, no checkpoint copies | 0.964 / 2.627 / 2.627 (7) | 4.639 / 5.323 / 5.329 (20) |
| 2 ticks, 2 observations, 32,715,088 checkpoint bytes | Not observed | 7.126 / 7.400 / 7.400 (4) |
| All sampled simulation windows | 2.487 / 2.799 / 4.888 (600) | 2.312 / 4.622 / 5.257 (600) |

- One observation dispatches **9,046 history workgroups**; two dispatch **18,092**. Captures copy **32,715,088 bytes (~31.2 MiB)** of particle/trail state during ordinary playback. In run 1, all twelve slowest frames advance **one tick** and capture that state, disproving a blanket attribution of those spikes to multi-tick catch-up. In run 2, the four slowest frames both advance two ticks and capture checkpoints. These matched observations identify separate checkpoint and repeated-observation workloads that last-phase percentiles cannot explain. They are **correlation, not an isolated copy-only timer or a causal A/B test**; grouping does not control live population or time within the volley. Small groups and changing tick mixes must not be treated as stable percentile certification. No performance optimization or named full-frame/finale budget completion is claimed by this instrumentation step.

```powershell
cargo test --locked -p aestra-bevy-render --lib simulation_timing -- --test-threads=1
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo test --locked -p aestra-viewer --bin aestra-viewer gpu_bench
cargo clippy --locked -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/frame-work-checkpoints-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --gpu-bench target/fireworks-f1/frame-work-checkpoints-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --sample-frames 240 --capture target/fireworks-f1/frame-work-volley-overlap
```

### Implemented — host-selectable playback-only checkpoint retention

- `PlaybackHistoryPolicy::{PlaybackOnly, ReplayEnabled}` is a transient, per-instance host API, independent of authored looping, seek quality and input-event history. Use `EffectPlayer::from_compiled(compiled).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly)` for game playback; inspect/change it with `history_policy` / `set_history_policy`. `PresentedEffect` exposes the same API for direct renderer hosts, and the engine-neutral `EffectInstance` and generic GPU `StageTimeline` carry it too. Nested effects inherit the root policy, including live changes, and hot replacement preserves it. `ReplayEnabled` remains the compatibility default; CPU scrub caching still requires `enable_scrub_cache`, which explicitly opts into replay retention.
- Playback-only skips automatic CPU scrub snapshots and GPU particle, analytic/stateful trail and extension/domain checkpoint allocation/copies. It does **not** disable live trail samples, authored events, or input retention. Switching off releases CPU caches immediately and GPU caches when the render preparation consumes the change, without resetting the clock, seed, epoch, revision, particles, domains or trails. Switching back allows future GPU captures rather than reconstructing missing snapshots. Explicit backward seeks still reset/reconstruct from zero when necessary; neither policy records historical live inputs, so exact reconstruction of those inputs requires a host-supplied trace. Playback-only is not a promise of cheap seeking or a smaller authored trail pool.
- Native regressions prove no snapshots/capture bytes throughout the budgeted 16-cohort volley (12,800 accepted children), unchanged trails/particles on a paused policy change, paged mixed analytic/stateful backward reconstruction without snapshots, resumed future captures, and domain state retained across a policy switch. CPU tests cover cache release, unchanged playhead/history identity, backward seeks and effect replacement; nested tests cover propagation without presentation recreation. Existing replay-enabled trail/seek regressions remain passing.
- Viewer `--history playback-only|replay-enabled` drives the actual root and children; benchmark JSON records the policy. Three sequential, isolated runs retain the prior RTX 4070 SUPER/Vulkan, 960×540, close camera, high tier, default-fast transparency, 120 warm-up/600 measured host-frame configuration. All report **600 unique complete-window samples**, **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, zero source overflow, omission, rejection, eviction or truncation, and the unchanged **38,580,664-byte configured-buffer estimate** (not total resident memory). Both playback-only runs report **zero checkpoint capture bytes in every sample**; the replay control has 30 capture-bearing samples, each copying **32,715,088 bytes**.

Frame-aligned simulation GPU timing in milliseconds; sample counts in parentheses:

| Work | Playback-only run 1 p50 / p95 / p99 | Replay-enabled control p50 / p95 / p99 | Playback-only run 2 p50 / p95 / p99 |
| --- | ---: | ---: | ---: |
| 1 tick, 1 observation, no checkpoint copies | 2.190 / 2.801 / 2.922 (547) | 2.523 / 2.799 / 2.848 (547) | 2.531 / 2.811 / 2.828 (598) |
| 1 tick, 1 observation, 32,715,088 checkpoint bytes | Not captured | 4.806 / 5.108 / 5.158 (28) | Not captured |
| All sampled simulation windows | 2.183 / 2.824 / 3.777 (600) | 2.515 / 2.822 / 5.024 (600) | 2.531 / 2.811 / 2.827 (600) |

- Removing snapshot work eliminates its periodic copy/allocation path while preserving produced work. Tick mixes vary: playback run 1 has 26 zero-/27 two-tick samples, control 12/13, playback run 2 1/1. Timing groups do not match exact populations/time, and the small multi-tick groups do not certify stable percentiles. These remain host-received timestamp windows, not a GPU-frame barrier or isolated copy-only timer. The live allocation/reservation/bounds kernels and catch-up cost remain; this does **not** close the named full-frame/finale budget gate.
- Playback-only frame 240 is byte-identical to the prior volley reference (SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`). Separate analytic 800-parent `trail-hero` captures at frames 30, 120 and 250 match the same-build replay-enabled control byte-for-byte, covering live/retired-tail output at those frames, not all assets/times.

```powershell
cargo test --locked -p aestra-runtime -p aestra-bevy --lib
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --lib playback_history_policy -- --test-threads=1
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo test --locked -p aestra-viewer --bin aestra-viewer
cargo clippy --locked -p aestra-runtime -p aestra-bevy-render -p aestra-bevy -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/playback-only-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history replay-enabled --gpu-bench target/fireworks-f1/history-replay-control-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/playback-only-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/playback-only-volley-overlap
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail-hero --camera close --backend gpu --history playback-only --sample-frames 30,120,250 --capture target/fireworks-f1/playback-only-analytic-trails
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe trail-hero --camera close --backend gpu --history replay-enabled --sample-frames 30,120,250 --capture target/fireworks-f1/history-replay-analytic-trails
```

### Implemented — birth-gated live trail candidate ordering

- Paged trail allocation now orders replacement candidates only when the GPU birth scan finds a head needing a new owner. Continuing heads use the existing owner mapping. Birth/eviction observations retain the same deterministic free-first, oldest-retired ordering and physical-owner tie breaks; owner-identity sorting, expiry, head ordering, sampling and bounds remain unchanged. The cooperative small-pool path is unchanged.
- The scan publishes its complete birth total in the first transient bounds-output word; candidate sorting/merging reads it before the bounds pass overwrites it with occupied counts. This adds **no buffers, persistent state, storage bindings, CPU readbacks or checkpoint retention**. Page sorting masks loads/comparisons/stores but retains uniform barriers for D3D/FXC portability; barrier-free candidate merge kernels return early. Dispatches are still encoded: one observation still counts **9,046 workgroups**, not fewer dispatched groups.
- Native GPU regressions poison both candidate-list ping-pong ranges after reservation on a no-birth observation, then verify they remain untouched while owner identities and world bounds stay valid. Initial/reset scans must publish the full birth count across pages. Coverage includes two emitters, strand ordering, identity extrema, partial/non-power-of-two pools (1,025 and 2,053 owners), 8,192 owners, retirement, expiry, deterministic eviction and restart. The loss-free volley and mixed analytic/stateful replay tests also pass. Generated WGSL validation/snapshot and HLSL/SPIR-V translation remain passing.
- Sequential RTX 4070 SUPER/Vulkan runs use playback-only, 960×540, close camera, high tier, default-fast transparency and 120 warm-up/600 measured host frames. A fresh control was built with the previous shader (always sorting candidates); optimized run 1 preceded that control, and run 2 followed it. All have **600 unique complete-window samples**, the same 1 zero-/598 one-/1 two-tick mix, **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, zero overflow/omission/rejection/eviction/truncation, zero checkpoint capture bytes and the unchanged **38,580,664-byte configured-buffer estimate** (not total GPU memory).

GPU timings in milliseconds; p50 / p95 / p99:

| Measurement | Previous-shader fresh control | Birth-gated run 1 | Birth-gated run 2 |
| --- | ---: | ---: | ---: |
| Complete simulation window (600 samples) | 2.540 / 2.836 / 2.970 | 2.263 / 2.618 / 2.855 | 2.384 / 2.654 / 2.875 |
| 1 tick, 1 observation, no checkpoint copies (598 samples) | 2.541 / 2.836 / 2.983 | 2.263 / 2.619 / 2.859 | 2.384 / 2.654 / 2.876 |
| Last-observation allocation/sample diagnostic (600 samples) | 0.741 / 0.829 / 0.885 | 0.443 / 0.536 / 0.833 | 0.472 / 0.539 / 0.846 |

- Allocation/sample p95 falls about **35%**; complete simulation p95 falls **6–8%** against the fresh control. Birth observations still pay candidate ordering, so high-percentile allocation spikes remain. Run-to-run drift and population/time differences are not controlled by grouping, diagnostic phase percentiles cannot be summed into a frame, and one two-tick sample cannot certify catch-up performance. This is a live-path improvement, **not completion of the full-frame/finale budget gate**.
- Optimized volley frame 240 remains byte-identical to the previous reference (SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`); this is output evidence at that frame, not an all-assets/all-times visual guarantee.

```powershell
cargo test --locked -p aestra-gpu --lib --test shader_contract --test trail_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --test-threads=1
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo clippy --locked -p aestra-gpu -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/birth-gated-candidates-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/birth-gated-candidates-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/birth-gated-candidates-volley-overlap
```

### Implemented — direct live head/owner matching

- Paged reservation no longer sorts the entire owner pool by stable ID. Head presentation clears the per-head owner map; a barrier-free owner-page pass expires old owners and binary-searches the already ordered `(strand, stable ID)` head list using its compact cached keys. Occupied matching owners reserve their existing physical slots before birth ranks/candidate allocation. The following reservation pass emits birth flags directly from the map. Kind 1 of the existing page entry point now means matching; planner tests require it to follow head presentation and have **no owner merge chain**. Head and replacement-candidate ordering, the eight-storage-binding ABI, scratch allocation, persistent history/checkpoint format and cooperative small-pool algorithm are unchanged.
- Matches have disjoint destinations: valid history has at most one owner per stable ID. Starting/reset history is empty; existing identities reserve their unique owners, and missing identities receive distinct deterministic candidate ranks with reserved owners excluded. This preserves uniqueness across retirement, expiry and recycled IDs. Matching reads the freshly generated head cache before candidate ordering reuses it, and reconstructs the map from persistent records every observation; no retained map or new playback/replay state is needed.
- Native regressions now poison both owner-list ping-pong ranges **before reservation**, proving matching does not consume or overwrite the removed owner-sort outputs. Continuing/no-birth observations also leave these ranges untouched through allocation. Reappearing retired identities with reversed compacted head order must reuse the same physical owner and preserve their sample rings. Existing two-emitter/strand/extreme-ID/partial-pool/8,192-owner eviction tests, the 16-cohort loss-free volley, sampling/bounds and replay/checkpoint reconstruction regressions pass. Generated WGSL validation/snapshot, HLSL/SPIR-V translation and checked range/planner tests pass.
- Three sequential runs on RTX 4070 SUPER/Vulkan use the same playback-only volley, 960×540 close camera, high tier, default-fast transparency and 120 warm-up/600 measured host frames. The fresh control uses commit `4057bea4`'s shader and planner (birth-gated candidates plus owner sorting); optimized run 1 precedes it and run 2 follows it. All retain **600 unique complete-window samples**, the 1 zero-/598 one-/1 two-tick mix, **37,600 captured/accepted children**, **7,201 peak live particles**, **10,400 occupied/3,200 retired histories**, zero overflow/omission/rejection/eviction/truncation, zero checkpoint capture bytes and the unchanged **38,580,664-byte configured-buffer estimate** (not total resident memory).
- Six owner-merge dispatches are removed per observation. Encoded history work falls from **9,046 to 5,974 workgroups** for this two-emitter fixture; two-observation windows fall from 18,092 to 11,948. These counts include over-dispatched/inactive groups and are not a count of useful owner operations. The source still visits owners to expire/match them; this is not a sparse active-owner allocator or GPU-indirect dispatch path.

GPU timings in milliseconds; p50 / p95 / p99:

| Measurement | Fresh owner-sort control | Direct-matching run 1 | Direct-matching run 2 |
| --- | ---: | ---: | ---: |
| Complete simulation window (600 samples) | 2.082 / 2.640 / 2.877 | 1.970 / 2.148 / 2.513 | 2.015 / 2.158 / 2.531 |
| 1 tick, 1 observation, no checkpoint copies (598 samples) | 2.082 / 2.641 / 2.881 | 1.970 / 2.148 / 2.516 | 2.015 / 2.158 / 2.532 |
| Last-observation reservation diagnostic (600 samples) | 0.551 / 0.723 / 0.729 | 0.205 / 0.221 / 0.224 | 0.206 / 0.222 / 0.225 |

- Reservation p95 improves about **69%**, complete simulation p95 about **18–19%** against the fresh control. Allocation/sample p99 is still 0.886/0.891 ms versus control 0.855 ms: birth candidate sorting remains a separate bottleneck. Sequential runs and matching aggregate demand/tick mix do not control exact populations/time or clock drift; diagnostic phase percentiles cannot be summed into a frame, and the single two-tick sample does not certify catch-up. The named **full-frame/finale budget gate remains open**.
- Optimized volley frame 240 is byte-identical to the prior overlap reference (SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`). This complements the native ownership/history tests but is not an all-assets/all-times visual guarantee.

```powershell
cargo test --locked -p aestra-gpu --lib --test shader_contract --test trail_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --test-threads=1
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo clippy --locked -p aestra-gpu -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/head-matched-owners-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/head-matched-owners-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/head-matched-owners-volley-overlap
```

### Implemented — live-prefix heads and top-K replacement candidates

- Head merges now read/write only the actual compacted live prefix, with every opposing-run search bounded to that prefix. Empty head pages skip payload initialization, key caching, comparisons and stores. Active partial pages still initialize their local sentinels, and passes beyond the live prefix's required merge depth still copy it to preserve the planned ping-pong parity. Page barriers and capacity-sized dispatches remain unchanged for D3D/FXC uniformity; this is not GPU-indirect dispatch or an active-page planner.
- Replacement merges retain only the first **K = birth count** candidates per run. The global first K cannot contain an element outside either input run's first K. Searches and writes respect those retained lengths, preserving free-first, oldest-retired and physical-slot tie-breaking. Discarded suffixes may be stale and are never read by subsequent merges/allocation. Candidate pages still sort their complete local page on a birth observation; no-birth gating remains. Storage bindings, configured scratch/history allocation, persistent history and checkpoint format are unchanged. No CPU readback, retained map or new replay state is introduced.
- Native conformance poisons inactive head pages and every discarded candidate suffix with invalid physical indices before consumers run, and compares presented live heads against a CPU `(strand, stable ID, physical slot)` ordering oracle before later ribbon linking can mask an error. Fresh allocation covers live/K counts **0, 1, 255, 256, 257, 511, 512, 513, 1023**, partial 1,025/2,053-owner pools and an 8,192-owner pool. Existing two-emitter/strand/extreme-ID/retired-identity reuse/oldest-retired eviction tests pass, together with the loss-free event volley, sampling/bounds, history-policy and replay/checkpoint tests. The GPU library/contracts (35 tests), native trail conformance (12), renderer trail regressions (11), viewer tests (37), generated WGSL snapshot/validation, HLSL/SPIR-V translation and all-target Clippy pass.
- New viewer probe **`event-trail-sparse`** changes only volley demand from 800 to 80 stars per cohort, apart from its display name/asset ID. It retains the dense probe's **8,192-particle / 16,384-history / 32-point** budgets, timing and geometry. A compile/normalized-RON test enforces that equality. This distinguishes low occupancy from an artificially smaller allocation.
- Six sequential RTX 4070 SUPER/Vulkan runs use playback-only, 960×540 close camera, high tier, default-fast transparency and 120 warm-up/600 measured host frames. Controls use commit `ccf4198e`'s shader with the same current viewer fixture; dense run order is optimized/control/optimized repeat, sparse order is control/optimized/optimized repeat. Compare each probe to its own control, not dense to sparse. Every run keeps **600 unique complete-window samples**, zero source overflow, expansion omission, destination rejection, trail eviction/truncation and checkpoint capture bytes. Dense runs retain **37,600 accepted/captured children**, **7,201 peak live**, **10,400 occupied/3,200 retired histories**; sparse runs retain **3,760 accepted/captured**, **721 peak live**, **1,040 occupied/320 retired**. All retain the **38,580,664-byte configured-buffer estimate** (not total resident memory) and **5,974 encoded history workgroups per observation**, including inactive groups; two-observation windows encode 11,948.

GPU timings in milliseconds; p50 / p95 / p99:

| Probe / measurement | Fresh control | Optimized run 1 | Optimized run 2 |
| --- | ---: | ---: | ---: |
| Dense complete simulation (600 samples) | 1.739 / 2.159 / 2.520 | 1.673 / 2.161 / 2.495 | 2.041 / 2.168 / 2.510 |
| Sparse complete simulation (600 samples) | 1.691 / 1.805 / 2.134 | 1.647 / 1.712 / 1.995 | 1.653 / 1.982 / 2.555 |
| Sparse 1 tick, 1 observation, no copies (598 / 598 / 571 samples) | 1.691 / 1.805 / 2.137 | 1.647 / 1.712 / 1.995 | 1.653 / 1.769 / 2.040 |
| Sparse last-observation head diagnostic (600 samples) | 0.480 / 0.514 / 0.521 | 0.433 / 0.449 / 0.456 | 0.433 / 0.463 / 0.467 |
| Sparse last-observation allocation/sample diagnostic (600 samples) | 0.417 / 0.466 / 0.800 | 0.419 / 0.458 / 0.717 | 0.420 / 0.456 / 0.726 |

- Dense simulation **p95 is essentially unchanged** and median/mean gains do not repeat; this does not establish a dense-volley improvement. Sparse head p95 is about **10–13% lower** and allocation/sample p99 about **9–10% lower**, with one-tick simulation p95 about **2–5% lower**. These are sequential observations, not paired population/time/clock-controlled measurements, and diagnostic percentiles cannot be summed into a frame.
- All dense runs and sparse control/run 1 have the 1 zero-/598 one-/1 two-tick mix. Sparse run 2 instead has **14 zero-/571 one-/15 two-tick windows**; its aggregate p95/p99 are worse than control despite the lower one-tick/phase timings. Do not discard those windows or claim overall catch-up latency improved. Its two-tick p95 is 3.016 ms, from only 15 samples, not a certification. The named **full-frame/finale budget gate remains open**.
- Optimized dense volley frame 240 remains byte-identical to the prior overlap reference (SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`). This is a targeted visual check, not an all-assets/all-times guarantee.

```powershell
cargo test --locked -p aestra-gpu --lib --test shader_contract --test trail_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --test-threads=1
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo test --locked -p aestra-viewer --bin aestra-viewer
cargo clippy --locked -p aestra-gpu -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/live-prefix-topk-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/live-prefix-topk-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-sparse --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/live-prefix-topk-sparse-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-sparse --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/live-prefix-topk-sparse-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/live-prefix-topk-volley-overlap
```

Fresh control reports are `target/fireworks-f1/live-prefix-topk-control-bench.json` and `target/fireworks-f1/live-prefix-topk-sparse-control-bench.json`, produced with only the paged shader restored to `ccf4198e` while keeping the new sparse probe.

### Implemented — blocked birth-rank scan; indirect-dispatch trials rejected

- The birth-rank scan now divides each 1,024-head page into 64 contiguous lane blocks. A lane retains its at-most-16 boolean birth flags in a register mask, scans only lane totals in shared memory, then writes each exclusive rank using its lane offset plus a population count of preceding bits. This avoids an intermediate aux write/read and reduces the full page's **tree barriers from 20 to 12** (not the total barrier count). Smaller power-of-two pages use one flag per lane. Lanes have disjoint destinations, inputs are the existing `u32(birth)` flags, and the page total/carry contract and stable birth order are unchanged. Barrier loop sizes still derive solely from read-only emitter metadata for FXC uniformity; no storage-backed early return is added.
- No new buffer, binding, entry point, dispatch, atomic allocation order, CPU readback or persistent/replay state is introduced. The existing candidate ordering and retired-owner expiry remain intact. Native fresh-allocation coverage now also includes **15/16/17, 63/64/65 and 1,024/1,025** heads, alongside empty, page-boundary, partial-pool, extreme-ID, strand, reserved/reappearing-owner and retired-eviction cases. GPU library/contracts (35), native trail conformance (12), coupled renderer trail regressions (11), WGSL snapshot/validation, HLSL/SPIR-V translation and all-target Clippy pass.
- A GPU-counted-dispatch prototype was tested first, then removed rather than enabled by default. It reduced launched head/scan groups, clamped each emitter's live count, ignored non-paged populations, handled empty frames and retained a direct fallback. But on the measured RTX 4070 SUPER/Vulkan path, dense **one-tick p95 rose from 2.168 to 2.333 ms** (598 control/535 prototype samples); sparse one-tick p95 rose from **1.769 to 1.846 ms** (598 each). A narrower prototype limited indirect execution to barrier-bearing head/birth/candidate pages and skipped candidate pages with no births; dense one-tick p95 was **2.224 ms** (587 samples), sparse **1.832 ms** (581 samples). These sequential measurements do not isolate indirect validation, copies or preparation as the cause. Neither prototype, its extra 72-byte dispatch storage nor its work-count/API changes remains in the implementation. Reports are `target/fireworks-f1/active-head-dispatch-{control,volley,sparse-control,sparse}-bench.json` and `active-page-dispatch-{volley,sparse}-bench.json`.
- Final register-mask measurements use the same high-tier/default-fast/playback-only, 960×540 close-camera probes, 120 warm-up/600 measured host frames and fresh pre-change `e2eb4e69` controls. Dense accepted/captured demand remains **37,600**, with **7,201 peak live / 10,400 occupied / 3,200 retired**; sparse demand remains **3,760**, with **721 / 1,040 / 320** peaks. Every control/final run reports zero overflow, omission, rejection, eviction, truncation and checkpoint capture bytes. Configured-buffer estimate stays **38,580,664 bytes** (not total resident memory); history dispatches stay **5,974 groups per observation**, 11,948 for two, including inactive groups.

GPU timings in milliseconds; p50 / p95 / p99:

| Probe / measurement | Fresh control | Register-mask run 1 | Register-mask run 2 |
| --- | ---: | ---: | ---: |
| Dense complete simulation (600 samples) | 1.993 / 2.167 / 2.512 | 2.043 / 2.154 / 2.501 | 2.105 / 2.171 / 2.523 |
| Dense 1 tick, 1 observation, no copies (598 / 598 / 587 samples) | 1.993 / 2.168 / 2.515 | 2.043 / 2.154 / 2.501 | 2.107 / 2.171 / 2.524 |
| Dense last-observation allocation/sample diagnostic (600 samples) | 0.474 / 0.537 / 0.868 | 0.479 / 0.537 / 0.862 | 0.498 / 0.530 / 0.865 |
| Sparse complete simulation (600 samples) | 1.671 / 1.769 / 2.042 | 1.667 / 2.028 / 3.517 | Not repeated |
| Sparse 1 tick, 1 observation, no copies (598 / 567 samples) | 1.671 / 1.769 / 2.057 | 1.667 / 1.757 / 2.049 | Not repeated |

- The lower barrier count is an algorithmic reduction, **not evidence of a meaningful overall latency improvement**: dense p95 is essentially flat and median/mean gains do not repeat. Dense control/run 1 use the 1 zero-/598 one-/1 two-tick mix; run 2 uses 6/587/7. Sparse control uses 1/598/1, final run uses **16/567/17**: its aggregate tail latency is worse despite a similar one-tick percentile. Preserve all windows rather than hiding catch-up spikes. Sequential runs are not paired population/time/clock controls; diagnostic phase percentiles cannot be added into a frame. The named **full-frame/finale budget gate remains open**.
- Final dense volley frame 240 is byte-identical to the prior reference (SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`). This remains a targeted visual check, not an all-assets/all-times guarantee.

```powershell
cargo test --locked -p aestra-gpu --lib --test shader_contract --test trail_contract
$env:AESTRA_REQUIRE_GPU_CONFORMANCE = '1'
cargo test --locked -p aestra-bevy-render --test trail_conformance -- --test-threads=1
cargo test --locked -p aestra-bevy-render --lib trail -- --test-threads=1
cargo clippy --locked -p aestra-gpu -p aestra-bevy-render -p aestra-viewer --all-targets -- -D warnings
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/masked-birth-scan-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/masked-birth-scan-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-sparse --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/masked-birth-scan-sparse-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/masked-birth-scan-volley-overlap
```

### Implemented — deterministic packed free-owner compaction

- Owner matching now processes four contiguous physical owners on each of its 64 lanes and packs their empty flags into one word in existing owner list 0. Expiry happens before classification; reserved live and unexpired retired owners cannot be free. Real owners are read once during matching; padded masks are explicitly zeroed. The existing birth-page dispatch counts these masks, scans 64 lane totals and, **only when empty slots cover all K births**, scatters the first K empty owners in ascending physical-slot order into list 1. Birth rank/strand ordering is unchanged, and the fast path explicitly consumes list 1 independently of shared merge parity.
- Sufficient empties bypass candidate key loads/comparisons and merge-list work. Insufficient empties retain the existing empty-first / oldest-retired / physical-slot-tie-break sorting path. There is no extra buffer, binding, entry point, dispatch, append atomic, CPU readback or persistent lookup; masks are rebuilt on every recording observation. **Capacity-sized dispatches and the sorting kernel's uniform barriers remain** for FXC portability, and the birth-page dispatch adds one 64-lane scan, including its barriers on birth-free observations. This is not an active-page dispatch reduction.
- Native coverage adds fragmented expired/reserved/unexpired-retired pools at **K = free−1, free, free+1**, with independent allocation/eviction oracles, preserved surviving sample rings, both strand orders, partial last blocks and 1,025/2,053/8,192-owner pools. The harness also deliberately uses a larger shared dispatch schedule to test local range guards and extra merge parity. Poisoned input lists are rebuilt without being read; birth-free candidate ordering preserves masks, unused padding and the output list. Existing prefix, extreme-ID, reappearing-ID, reset, pause, spatial-sampling, 64-point-ring, replay and loss-free event-volley regressions remain green. **95 targeted tests** (35 GPU library/contracts, 12 native conformance, 11 coupled renderer trail tests, 37 viewer tests), all-target Clippy and formatting pass.
- The first prototype serially reread strided history records in the birth-page workgroup and raised dense allocation/sample diagnostic p99 from **0.860 to 1.075 ms**. Classifying compact flags during matching reduced that to 0.915 ms; packing four flags per word reduced it further to **0.706 / 0.712 ms** in two dense runs. Only the packed implementation remains. Intermediate reports are `free-owner-volley-bench.json` and `free-flags-{volley,sparse}-bench.json` under `target/fireworks-f1/`.
- Fresh controls are from `f7deec36`. Final measurements use the same RTX 4070 SUPER/Vulkan, high-tier/default-fast/playback-only, 960×540 close-camera probes and 120 warm-up / 600 measured host frames. Every control/final run retains dense **37,600 captured = accepted**, **7,201 live / 10,400 occupied / 3,200 retired** peaks, or sparse **3,760 captured = accepted**, **721 / 1,040 / 320** peaks, with zero overflow, omission, rejection, eviction, truncation and checkpoint capture bytes. Configured-buffer estimate stays **38,580,664 bytes** (not total resident memory); launched history groups stay **5,974 per observation** / 11,948 for two.

GPU milliseconds; p50 / p95 / p99:

| Probe / measurement | Fresh control | Packed run 1 | Packed run 2 |
| --- | ---: | ---: | ---: |
| Dense complete simulation (600 samples) | 2.071 / 2.163 / 2.512 | 2.013 / 2.330 / 3.998 | 2.035 / 2.183 / 3.909 |
| Dense 1 tick, 1 observation, no copies (598 / 565 / 584 samples) | 2.071 / 2.163 / 2.515 | 2.013 / 2.179 / 2.374 | 2.035 / 2.177 / 2.364 |
| Dense last-observation allocation/sample diagnostic (600 samples) | 0.499 / 0.528 / 0.860 | 0.479 / 0.538 / 0.706 | 0.488 / 0.539 / 0.712 |
| Sparse complete simulation (600 samples) | 1.645 / 1.757 / 2.042 | 1.670 / 1.772 / 1.895 | Not repeated |
| Sparse 1 tick, 1 observation, no copies (598 each) | 1.645 / 1.757 / 2.050 | 1.670 / 1.772 / 1.897 | Not repeated |
| Sparse last-observation allocation/sample diagnostic (600 samples) | 0.407 / 0.459 / 0.724 | 0.427 / 0.462 / 0.559 | Not repeated |

- The allocation/sample diagnostic p99 reduction repeats in dense runs, but median/p95 gains are small or absent; **do not claim an overall p95 win**. Dense control has 1 zero-/598 one-/1 two-tick frames, versus **17/565/18** and **8/584/8** in packed runs. All-frame tail latency is worse with the extra catch-up windows (two-tick p95 4.360 / 4.183 ms, only 18 / 8 samples). Sparse control/final both have 1/598/1. Sequential runs are not paired population/time/clock controls, and a last-observation diagnostic is not a complete frame or a birth-only timing. Keep every frame window visible; the **full-frame/finale performance gate remains open**.
- Dense frame 240 is byte-identical to the prior reference: `target/fireworks-f1/packed-free-volley-overlap/frame-000.png`, SHA256 `4B45502997252D44EE17DB4C64FB0950691A987CF82981DE0FE21199B6E2F051`. This remains a targeted visual check, not an all-assets/all-times guarantee.

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/packed-free-volley-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/packed-free-volley-repeat-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-sparse --camera close --backend gpu --history playback-only --gpu-bench target/fireworks-f1/packed-free-sparse-bench.json
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera close --backend gpu --history playback-only --sample-frames 240 --capture target/fireworks-f1/packed-free-volley-overlap
```

Control reports: `target/fireworks-f1/free-owner-control-bench.json` and `free-owner-sparse-control-bench.json`, generated before the shader change at `f7deec36`. Verification commands are the preceding section's commands, plus `cargo test --locked -p aestra-viewer --bin aestra-viewer`.

### Tasks

- Next: add **controlled playback-only benchmark tick schedules** for reproducible one-tick and multi-tick catch-up comparisons, retaining the existing real-time/all-frame reports separately. Advance the live runtime rather than opting into replay; pair each timing window with demanded/accepted births, live/retired populations and history work. Use work-matched repetitions to isolate the remaining head-ordering, matching, uniform-barrier and catch-up costs before another optimization. Do not optimize editor replay ahead of playback.
- Measure variable multi-tick catch-up with enough work-matched samples. Revisit GPU-counted dispatch only with an encoding plan whose preparation/copy/validation costs are demonstrated to pay for themselves; fewer launched groups alone was not a win. Certify the named full-frame budget on measured hardware before claiming the trail scale gate complete.
- Establish thousands-of-trails overlapping-shell/finale benchmarks and investigate aggregate high-percentile spikes before declaring the named full-frame budget met.
- Replace single-buffer/record ceilings with checked, device-aware resource planning and chunking where needed.
- Make active/retired tails, evictions, truncation, memory and per-pass cost visible in the profiler.
- After the live path meets its budget, retest opt-in checkpoint storage and seek latency for editor scrubbing; scale cadence/storage policy from measured memory without charging unnecessary replay work to playback-only hosts.

### Deliverable and exit gate

One 300–800-star emitter compiles and renders a matching trail per star, including unexpired retired tails, and passes the section 8 correctness and budget tests. Thousands-of-trails volley and finale benchmarks are established before F3, even if final tier tuning continues in F9.

---

## Milestone F2 — Generic velocity/distribution system

**Goal:** remove the largest remaining shell-shape limitation.

### Implementation status — 2026-10-01

The native foundation for F2.1–F2.4 is implemented:

- `VelocityDistribution` provides Constant, Cone, Sphere, Hemisphere, Disk and Ring, plus `LegacyCone` for unchanged existing assets. Missing source fields default to legacy sampling and are omitted when saved in that mode; analytic and stateful legacy samplers retain their original behavior.
- `ModuleInstance::initialize_with_distribution(...)` authors a local axis, a velocity mode, an independent speed range and a full Cone opening angle in degrees. Spawn position stays in the separate Shape module. Sphere/hemisphere/cone are uniform in solid angle; Ring is uniform in azimuth and preserves sampled speed; Disk is uniform in planar area and scales sampled speed by `sqrt(u)` to fill the disk.
- The compiler carries the typed mode into Initialize instructions and GPU artifacts. The mode is a **compile-time choice**, not a live text parameter: attempts to bind it are diagnosed rather than silently ignored. Axis, spread and speed retain their existing input/binding paths.
- CPU analytic/stateful sampling and GPU analytic/ordinary/event-linked/domain-linked spawning use corresponding direction channels **30/31**, independent of position, speed, lifetime, drag and turbulence streams. RNG contracts remain backend-specific; this does not claim analytic and stateful seeds produce the same per-particle trajectories. Native CPU/GPU conformance checks each backend pair.
- The GPU emitter uses its former direction-padding lane for the mode, without changing emitter size or adding a storage binding. Stateful params append one mode word (188); a mode edit participates in the dynamics fingerprint and invalidates stale simulation/checkpoint history.
- The editor exposes an undoable Velocity distribution choice with Axis and Speed controls, and shows Spread only for Cone/Legacy Cone. Both locale catalogs describe the semantics. The existing XYZ axis controls provide orientation; a dedicated orientation gizmo is not part of this slice.
- Compiled artifacts are version **5**, so older readers reject/recompile instead of silently dropping new distribution semantics. Source effects remain format 4. Tests cover old-source defaults, all-mode authored/compiled round-trips, semantic edits/undo/redo, geometry/density, seed determinism and independent position/velocity streams.
- The viewer supplies `f2-peony`, `f2-ring`, `f2-palm`, `f2-hemisphere-fan` and `f2-double-ring` via the existing `--fireworks-f0-probe` entry point. They are deterministic **distribution sketches**, not production shell assets or substitutes for the F1/F1B workload gates.

Validation and remaining acceptance:

- CPU/core/compiler/authoring/artifact suites and native GPU analytic, death/reuse, checkpoint replay and event-linked birth conformance are covered by automated tests; shader validation/snapshots and editor semantic choice/undo are also checked.
- All five native GPU viewer sketches were captured and inspected in `target/fireworks-f2/`: sphere/peony, planar ring, narrow cone/palm, hemisphere fan and intersecting tilted rings. They deliberately do not include production trails, smoke, lighting or host sound. The editor choice/undo test passes; live editor controls still need manual visual acceptance before signing off F2's complete visual deliverable.
- Axis normalization is tested for zero, tiny and extreme finite axes, including a GPU-safe rescale that avoids subnormal-reciprocal NaNs. Strict Clippy (`-D warnings`), formatting and shader contracts pass.
- Keep F1B open for the named hero/finale full-frame budget and device-sized resource certification. F2 functionality is not an AAA-performance certification; F3 production entry gates below remain unchanged.

Example visual check (substitute any of the five probe names):

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f2-ring --camera close --backend gpu --history playback-only --sample-frames 30,60,90 --capture target/fireworks-f2/ring
```

### Phase F2.1 — built-in direction distributions

Implement at minimum:

```text
Constant
Cone
Sphere
Hemisphere
Disk
Ring
```

### Phase F2.2 — decouple position and velocity shape semantics

Ensure an emitter independently expresses:

```text
spawn position distribution
initial velocity direction distribution
speed distribution
```

### Phase F2.3 — semantic authoring/UI

A compact editor might present:

```text
INITIAL VELOCITY
Direction     Ring
Orientation   X/Y/Z or gizmo
Speed         18 – 22 m/s
```

### Phase F2.4 — deterministic CPU/GPU parity

- identical sample dimensions/random streams;
- compiler support;
- GPU shader support;
- CPU reference support;
- conformance tests.

### Later optional forms

- spline/curve distribution;
- mesh vertices;
- mesh surface;
- texture/point set;
- vector field;
- custom plugin distribution.

### Deliverable

Peony, ring and palm can all be authored generically.

---

## Milestone F3 — First production shell library

**Goal:** stop working only on abstract systems and validate the actual result.

**Entry gate:** F1's single-link burst and F1B's single-emitter trail workloads compile, run, replay and stay within the named Test A budget. A small sketch may precede these gates; a production hero shell may not be declared complete without them.

### Prototype implementation — 2026-10-01

- Editable source effects `assets/test/effects/fireworks_peony.aestra.ron` and `fireworks_chrysanthemum.aestra.ron` share the three procedural semantic materials `assets/test/materials/fireworks_{star,trail,smoke}.aestra.material.ron`. They resolve through the normal project loader, not viewer-injected material definitions, and can be loaded by a host through the existing project/compiler APIs.
- Each is a seven-second, one-shot shell: a single launch particle with a cooling trail; one actual launch-death event produces **256 stars through one link**, one short flash and 24 smoke puffs through two other links. Sphere velocity, independent speed/lifetime ranges, gravity/drag and appearance curves drive expansion and cooling. Chrysanthemum adds a bounded 64-point/256-owner trail pool to the same star emitter; Peony keeps clean star heads.
- The short launch smoke plume is an independent authored approximation, not smoke attached to live rocket positions. Burst smoke uses particle sprites, not a fluid domain; lighting, HDR/bloom and smoke realism are not certified by this slice.
- Launch speed and star speed are exposed Range parameters. Stable source/curve/renderer/material IDs and the F0 seed make the fixtures reproducible; ordinary effect placement, seed selection and `PlaybackHistoryPolicy::PlaybackOnly` remain host-controlled. No firework-specific runtime type was added.
- The visual slice revealed that native stateful ordinary emission ignored authored `burst_count`. The generic dispatch now carries one-shot birth count/timing, suppresses autonomous births on event targets, honors emission stop cutoffs and fingerprints count/timing edits. A native production-loop regression checks initial and delayed bursts, one real child cohort, no repeated launch, exact backward-seek replay and stop suppression. This fixes **authored one-shot emission**, not every live host burst/UI interaction.
- Viewer entry points are `--fireworks-f0 --fireworks-f0-probe f3-peony` / `f3-chrysanthemum`; choose `--camera wide --backend gpu --history playback-only`. Source generation/round-trip, semantic material compilation, normal project resolution and CLI preparation are regression-tested.
- Native playback-only captures at frames 45/80/110/150/210/300 were inspected in `target/fireworks-f3/{peony-fixed,chrysanthemum-fixed}`: launch, flash, expansion, star-trail arcs and decay are present. Both telemetry reports show peaks of one launch, 256 stars, one flash and 24 burst-smoke particles. These observed peaks are not a full requested/produced-work or performance certification. Viewer tests (41 passed, fixture-export helper ignored), project suites, the native authored-burst regression, fingerprint tests, strict Clippy and formatting pass. The wide-view red Peony is visibly dim; HDR response and visual/material tuning remain necessary.

### Pistil / Willow prototype extension — 2026-10-01

- Added editable `assets/test/effects/fireworks_pistil.aestra.ron` and `fireworks_willow.aestra.ron`, using the same three project-resolved semantic materials and ordinary effect/compiler/host APIs. No runtime or shader specialization was needed.
- **Pistil:** the launch death drives a 256-star blue outer shell and a separate 96-star gold inner layer, alongside flash and smoke. Both layers inherit the same origin/velocity contribution but have non-overlapping speed ranges (18–22 and 8–10); the inner speed is also exposed. Its six emitters/four links are bounded pools, not an event-limit workaround.
- **Willow:** 256 gold stars live for 4.2–5 seconds with authored gravity/drag and 1.4-second cooling trails. Time sampling at 30 Hz uses 64 points per owner, enough to retain the visible window plus its boundary sample. A nine-second root/target window includes star death and retired-history decay; the independent launch plume still emits only for 1.3 seconds. This is a single cohort with 256 star owners, not overlapping-volley certification.
- CLI probes `f3-pistil` and `f3-willow` load the checked-in sources through normal project resolution. Regression tests cover all four fixture round-trips, material compilation, project/CLI preparation, bounded event targets without autonomous births, Pistil speed/color separation and Willow history/playback-window coverage. Viewer suite: **43 passed, one ignored fixture-export helper**; strict viewer Clippy and formatting pass.
- Inspected native GPU **playback-only** captures in `target/fireworks-f3/pistil` (frames 45/80/110/150/210/300) and `target/fireworks-f3/willow` (45/80/150/240/330/390/450/510). Pistil telemetry peaks at 256 outer and 96 inner stars; Willow peaks at 256 stars. Both show one launch/flash and 24 burst-smoke particles. Willow's final report has zero alive particles, occupied/retired trails and submitted geometry, with zero reported trail truncations/evictions. These are endpoint telemetry and visual checks, not per-frame budget certification. Pistil's wide-view star heads are still dim; Willow's expansion, falling arcs and cooling tail decay are visible but require reference/HDR tuning.

### Exposed cooling gradients — 2026-10-01

- All four sources expose **Launch color**, **Star color**, **Flash color** and **Smoke color** as ordinary `Gradient` parameters; Pistil additionally exposes **Pistil color** independently of its outer shell. The two smoke emitters share one control. Defaults preserve the authored gradients, including their cooling keys; parameter gradients have stable IDs distinct from module-local gradients. Particle heads and trail history use the same appearance input rather than separate material tints.
- Hosts use the existing `EffectPlayer::set_parameter(id, Value::Gradient(...))` and `clear_parameter(id)` APIs; no shell-specific setter, material program change or runtime feature was added. Set per-shell variations **before playback** for game workloads. Existing parameter/history invalidation semantics still apply to later edits; this slice does not claim cost-free live recoloring of retained trails or unlimited GPU gradient key counts (the fixtures use at most three keys within the existing eight-key budget).
- Regression tests cover preserved defaults, source/project compilation, typed host overrides and clear, independent layers/instances, unchanged GPU storage budgets and playhead/history policy, and analytic CPU plume color without trajectory changes. The stateless CPU evaluator does not produce the stateful event-born star cohorts. A required-hardware native GPU production-loop test separately checks gradient interpolation on real event-born particles, unchanged simulation/birth counters when re-presenting the same tick, and byte-exact presentation restoration. Viewer suite: **45 passed, one ignored fixture-export helper**; strict viewer/render Clippy and formatting pass.
- Default native GPU playback-only capture `target/fireworks-f3/chrysanthemum-color-controls` uses frames 45/80/110/150/210/300. SHA-256 comparison of all six frame PNGs against `target/fireworks-f3/chrysanthemum-fixed` is byte-identical: exposing the defaults did not change that shell's captured appearance. Project tests also pass. No editor manual acceptance or production timing certification is implied by these checks.

Example host-side variation after compiling/resolving the project and constructing its player:

```rust
use aestra_bevy::{ColorKey, Gradient, Value};

let star_color = player.effect().parameters.iter()
    .find(|parameter| parameter.name == "Star color")
    .expect("F3 shell exposes Star color")
    .source;
player.set_parameter(star_color, Value::Gradient(Gradient::new(vec![
    ColorKey::new(0.0, [0.1, 1.0, 0.2, 1.0]),
    ColorKey::new(0.6, [0.02, 0.5, 0.1, 1.0]),
    ColorKey::new(1.0, [0.0, 0.08, 0.02, 1.0]),
])))?;
// player.clear_parameter(star_color)?; // restores this shell's default gradient
```

These are **bounded-count visual prototypes, not completed production shells**. Even Pistil's 352 stars do not certify the named Test A workload, finale budgets, or F1/F1B's resource/performance gates. Still open: reference-footage comparison and material/curve tuning, editor manual acceptance (including exposed-gradient editing), and production-scale timing/memory certification. The next visual slice is F4's HDR/exposure/bloom response; production certification remains a separate gate.

Example deterministic capture:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-chrysanthemum --camera wide --backend gpu --history playback-only --sample-frames 45,80,110,150,210,300 --capture target/fireworks-f3/chrysanthemum
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-pistil --camera wide --backend gpu --history playback-only --sample-frames 45,80,110,150,210,300 --capture target/fireworks-f3/pistil
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-willow --camera wide --backend gpu --history playback-only --sample-frames 45,80,150,240,330,390,450,510 --capture target/fireworks-f3/willow
```

Build:

1. Peony.
2. Chrysanthemum.
3. Pistil.
4. Willow.

### Required components

- launch shell;
- launch trail;
- launch smoke;
- burst flash;
- main star materials;
- trail materials;
- color gradients;
- parameter exposure.

### Acceptance review

Compare against slow-motion/reference footage qualitatively for:

- expansion timing;
- arc shape;
- trail persistence;
- brightness hierarchy;
- star density;
- decay/falloff;
- asymmetry/variation.

No new firework-specific core concepts are allowed unless a generic need is demonstrated.

---

## Milestone F4 — HDR preview / photographic response

**Goal:** make physically bright effects read correctly.

### F4A implemented — fixed viewer photographic response (2026-10-01)

- Opt-in `--hdr`, `--exposure -8..8`, `--tonemapping tony|aces|reinhard`, and
  `--bloom 0..1`. Any photographic option selects an HDR intermediate camera target;
  legacy camera defaults and existing visual references are unchanged.
- The default photographic profile uses Tony McMapface, fixed 0-stop exposure,
  natural energy-conserving bloom (0.15, zero threshold), and disabled deband dithering.
  Zero bloom omits the bloom pass. There is no adaptive/auto exposure.
- Exposure uses scene-wide `ColorGrading.global.exposure`, not PBR-only camera
  `Exposure`: it affects the custom unlit Aestra particle/material paths too. These are
  relative display stops, not a calibrated physical EV100 camera. Bloom precedes display
  exposure; exposure does not change bloom's source radiance or simulation.
- `preview-report.json` records `capture.response` as additive schema-1 metadata:
  HDR, stops, display transform, bloom strength/preset and dither policy. Older reports
  without this field represent legacy defaults. Captures remain tonemapped SDR PNGs.
- Tests cover component placement, zero-bloom behavior, finite/bounded CLI values,
  order-independent options, report metadata, and identical compiled effects/seeds/history/
  sampled frames with and without the photographic controls.

Validation command:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-chrysanthemum --camera wide --backend gpu --history playback-only --hdr --exposure 2 --tonemapping tony --bloom 0.15 --sample-frames 45,80,110,150,210,300 --capture target/fireworks-f4/chrysanthemum-photo
```

Native GPU capture shows a luminous burst and brighter cooling trails using the **same**
F3 chrysanthemum asset and seed. Two independent photographic captures have byte-identical
PNGs at all six sampled frames on the tested adapter. This is a same-adapter repeatability
check, not a cross-GPU bit-exact guarantee or production-performance certification.
The new legacy capture also matches the prior F3 color-control baseline byte-for-byte at
all six frames; compiler metadata is identical, peak particles remain 310 and trail evictions
remain zero. A native 2D Prism Bloom capture exercises ACES, -1 stop and disabled bloom.
Viewer tests (49 passed, one ignored fixture exporter), strict all-target Clippy, formatting
and diff checks pass.

### F4B implemented — editor controls and shared host profile (2026-10-01)

- The viewer and editor use one serializable `PhotographicPreview` profile in the shared
  renderer, publicly available to hosts through `aestra_bevy::preview`. It owns fixed stops,
  the display transform and natural bloom strength; application normalizes invalid values.
- Settings → Preview offers an opt-in HDR toggle, exposure (-8 to 8), bloom (0 to 1), and
  Tony/ACES/Reinhard selector. Numeric controls use bounded steps of 0.1 stops / 0.01 bloom.
  Labels and descriptions are localized in English and French.
- Settings format 5 persists the profile. Older settings migrate without enabling HDR or
  changing existing grid/autoplay preferences. Disabling HDR retains the saved profile but
  restores the effect camera's legacy 3D response.
- Only `PreviewRenderCamera` receives the profile. Thumbnails, material-node previews and
  their cameras remain untouched. Gizmo/UI cameras stay LDR, clearing their separate intermediate
  textures to transparent and alpha-compositing over the HDR viewport; otherwise those LDR
  textures overwrite/hide the particles. Turning HDR off restores legacy composition.
  Camera changes do not recompile or restart an effect;
  new/restored viewport cameras receive the active profile too.
- The viewer's editor-viewport layering smoke mode now accepts the same photographic
  options, applying them only to its effect camera, not its overlay/probe cameras.
- Exposure/bloom scrubbing updates the camera continuously without rebuilding the settings UI
  or saving to disk on every pointer event; the final event persists the profile.

Verification: four editor photographic tests, 22 settings-related tests, nine localization tests,
three shared-profile tests and 48 viewer tests pass (one fixture exporter remains ignored).
Strict all-target Clippy for editor/viewer/renderer/Bevy client and formatting/diff checks pass.
The native GPU layering smoke passes at three frames with **both** an LDR UI camera and gizmo
overlay after the HDR effect camera, with no particles in the isolated layer-15 probe:

```powershell
cargo run --locked -p aestra-viewer -- --backend gpu --semantic-materials --hdr --exposure 2 --editor-viewport-smoke target/fireworks-f4/editor-photo-ui-composited --frames 3
```

The six-frame chrysanthemum capture using the shared profile is byte-identical to F4A's
photographic baseline. Full interactive editor acceptance (scrubbing controls, changing display
transforms, restarting with saved preferences, narrow layouts in both locales) remains a manual
check; the native smoke models camera composition rather than driving the full editor UI.

### F4C implemented — authored HDR radiance controls (2026-10-01)

- The four editable shell assets expose **Star radiance** (default 8) and **Trail radiance**
  (default 4) as stable-ID scalar effect parameters. Existing semantic material effect bindings
  resolve them through normal project compilation and host `EffectPlayer::set_parameter` /
  `clear_parameter`; no firework-specific runtime feature or shader specialization was added.
  Star gain also covers launch/flash and Pistil's inner stars; trail gain is independent.
- Shared star/ribbon graphs multiply linear `ParticleColor` by a gain clamped to 0..64.
  This bound is an editable **asset policy**, not an engine-wide HDR ceiling. Gains are artistic
  scene-RGB units, not calibrated watts. Alpha remains particle opacity times the procedural
  mask, with no dependency on gain; smoke materials, cooling gradients, sizes, widths, event
  counts and dynamics are unchanged. Material defaults remain 1 for older unbound instances.
- Game hosts should choose variants before playback. Runtime material binding refresh reuses
  the compiled program, but this does **not** promise cost-free arbitrary live effect-parameter
  edits: existing parameter/history invalidation semantics still apply. `MaterialBindingContext`
  is now re-exported by `aestra_bevy` for low-level hosts alongside `MaterialRuntimeBinding`.
- Regression coverage walks RGB/alpha expression dependencies, checks the asset clamp and
  unit fallback, compiles the generated shader with unclamped RGB, resolves independent gains
  from real compiled effect instances (including zero, unit and out-of-policy inputs), restores
  defaults, and checks unchanged GPU dynamics inputs. All four source fixtures round-trip and
  resolve through the ordinary project loader.

Native Vulkan captures on **RTX 4070 SUPER**, 960×540, fixed F0 seed, playback-only history,
Tony tonemapping, **0 stops** and natural bloom 0.15:

- Wide: `target/fireworks-f4/{peony,chrysanthemum,pistil,willow}-radiance`.
  Frames 45/80/110/150/210/300; Willow additionally samples 390/450.
- Chrysanthemum audience/close: `target/fireworks-f4/chrysanthemum-radiance-{audience,close}`,
  with the same six frames and response settings.
- All six reports succeed on native GPU, with zero reported trail evictions/truncation.
  Estimated peak particles remain 310 (Peony/Chrysanthemum/Willow) and 406 (Pistil).
  These are bounded prototype observations, not full requested/produced-work or budget gates.
- Contact sheets show luminous star cores and cooling golden ribbons; Peony's red and Pistil's
  blue/gold layering remain distinguishable after the early bright phase. Wide late stars still
  become dim, close framing clips the initial burst, and smoke remains an unlit approximation.
  These observations are not reference-footage/AAA acceptance or cross-GPU determinism proof.

Reproduce the wide chrysanthemum capture:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-chrysanthemum --camera wide --backend gpu --history playback-only --hdr --exposure 0 --sample-frames 45,80,110,150,210,300 --capture target/fireworks-f4/chrysanthemum-radiance
```

Verification: viewer suite **49 passed, one ignored fixture exporter**, project suites,
strict viewer/Bevy-client all-target Clippy and formatting/diff checks pass. The shell source
appearance now intentionally differs from F4A/B's unit-radiance captures; their recorded images
remain historical controls, not the new golden reference. Interactive editor acceptance remains open.

### F4D implemented — opt-in subpixel additive sprite sampling (2026-10-01)

- Generic host resource `aestra_bevy::SpriteSampling { minimum_pixels: 2.0 }`, default 0,
  controls **native GPU additive Sprite** presentation only. Finite values normalize to 0..8
  physical main-pass pixels; non-finite values disable it. The bound is a quality policy,
  not an engine-wide effect/particle constraint. No authored asset or compiler format changes.
- The shared vertex path projects both rotated quad axes through each camera's unjittered
  matrix and main-pass viewport. Below the shorter-axis floor, geometry expands uniformly
  and final fragment alpha is attenuated by inverse expanded area. The shared helper is used
  by legacy and semantic materials, including constant-alpha programs. Authored ParticleOpacity
  remains unchanged; gain, state, emission, trails and playback/replay histories are untouched.
  Renderer-resource changes apply without rebuilding particle buffers or restarting playback.
- Flipbooks, meshes, ribbons, trails and CPU reference/readback presentation are excluded.
  Zero/degenerate footprints are not resurrected; resolved quads are unchanged. Wireframe is
  still diagnostic shading. No extra passes, sample history or per-particle CPU work are added.
  Projection is exact for standard camera-facing perspective/orthographic quads; custom
  projections use a center Jacobian. This preserves **continuous footprint area × alpha**,
  not exact pixel-integrated radiometry or shimmer-free arbitrary material masks.
- Expanded screen footprints can exceed ordinary world AABBs. Qualifying draws therefore
  bypass CPU frustum culling while retaining raster clipping; turning off the policy restores
  ordinary sprite culling. This adds potential offscreen submissions/fill cost, not a free
  performance guarantee. Production/finale and temporal-quality gates remain open.
- Viewer option `--sprite-min-pixels 0..8` does not enable HDR. Nonzero policies reject explicit
  CPU/readback modes; automatic CPU fallback ignores it, so inspect the selected backend.
  Schema-1 reports add normalized requested `capture.response.sprite_minimum_pixels` (absent
  in old reports means disabled). Editor defaults have not been changed by this host/viewer slice.

Validation:

- Native GPU compute/raster regression exercises the actual shared shader: perspective and
  orthographic projection, viewport scaling, behind-camera/degenerate geometry, disabled/resolved
  cases and inverse-area identity. Sixteen isolated quarter-pixel sprites at distinct X/Y pixel
  phases exhibit untreated dropout; 2- and 4-pixel floors keep all tested phases nonzero, with
  each cell's alpha sum bounded by the original square footprint area. Legacy and constant-alpha
  semantic paths pass; zero-size geometry remains invisible and 4-pixel sprites render identically
  with the policy on/off. This bounded test is not a universal material/temporal certification.
- Policy normalization/excluded renderer kinds, reversible culling, viewer CLI/no simulation
  change and report metadata are covered. Shader snapshots and varying/fingerprint goldens
  explicitly include the new presentation coverage scalar. Portable WGSL/SPIR-V/HLSL contracts
  and native legacy/semantic/mesh/MSAA stage linking pass; viewer suite: **50 passed, one ignored**.
- RTX 4070 SUPER Vulkan, 960×540, fixed F0 seed, playback-only, Tony, 0 stops, bloom 0.15:
  `target/fireworks-f4/chrysanthemum-sampling-off` is byte-identical to all six F4C baseline PNGs.
  `target/fireworks-f4/chrysanthemum-sampling-2` uses the same frame sequence and records floor 2.
  Wide `target/fireworks-f4/{peony,pistil,willow}-sampling-2` also succeed on native GPU;
  Willow includes late frames 390/450. These are prototype integration captures, not AAA references.
  Native HDR multi-camera viewport smoke (`target/fireworks-f4/sampling-viewport-smoke`) passes
  three frames with floor 2: particles remain visible in the preview and absent from the overlay probe.
  Still captures exercise integration, not an animated shimmer/performance acceptance gate.

Strict all-target Clippy for GPU/renderer/Bevy client/viewer and formatting/diff checks pass.

Reproduce the treated wide capture:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-chrysanthemum --camera wide --backend gpu --history playback-only --hdr --exposure 0 --sprite-min-pixels 2 --sample-frames 45,80,110,150,210,300 --capture target/fireworks-f4/chrysanthemum-sampling-2
```

### F4E implemented — temporal sampling gates and reproducible presentation benchmarks (2026-10-02)

- Native raster regression now sweeps 16 isolated quarter-pixel stars over 32 pixel-phase steps,
  at 0 and 0.65-radian orientations, through both the legacy and constant-alpha semantic shaders.
  It checks finite/nonnegative sums, treated footprint bounds, no treated dropouts, reduced
  brightness modulation and phase-averaged agreement (within 5%) with the analytic integral of
  the procedural smoothstep-feathered circular mask. This is a bounded pre-postprocess probe,
  not a universal guarantee for textures, trails or a complete animated fireworks scene.
- RTX 4070 SUPER Vulkan observations, **512 samples per orientation/path/policy**:
  untreated quads drop out in **488 samples**, with relative standard deviation 4.65–4.66;
  floor 2 has **zero dropout**, relative deviation 0.267; floor 4 has **zero dropout**, deviation
  0.0674. Treated mean alpha is 0.03984–0.03989 versus continuous integral 0.039858.
  Legacy/semantic results agree. The test enforces the improvement and continuous-average gates,
  not exact cross-adapter floating-point values. A 2-pixel floor still has visible modulation in
  this probe; 4 pixels is a quality option, not a free or automatically enabled default.
- Benchmark JSON adds requested `presentation` (seed, camera, HDR/exposure/tonemapping/bloom,
  sprite floor, tier, budget, render mode and ordering), detected `adapter`, measured-window
  `effect_backends` sets and `physical_window_sizes`. Per-effect fallback is not confused with
  the global device decision; unknown data remains null/empty, not a fabricated successful
  native run. Multiple backends or sizes expose a mixed window. Player hotkeys cannot change
  playback, seed or shading while a benchmark runs. Existing timing/work scope is unchanged.

Playback-only repeated-volley observations (`event-trail-volley`), fixed F0 seed, wide,
960×540, high tier, native Vulkan on RTX 4070 SUPER, HDR Tony/0 stops/bloom 0.15, fast ordering,
**120 warm-up + 600 measured viewer frames**:

- Reports: `target/fireworks-f4/sampling-volley-{0,2}-bench.json`.
- Both report peak live particles **7,201**, occupied trails **10,400**, retired trails **3,200**,
  estimated buffer bytes **38,580,664**, no source-event overflow, expansion omission,
  destination rejection, history eviction or truncation. Observed link demand/acceptance is
  **37,600** in both runs; these asynchronous totals include warm-up, not just the measured window.
- Transparent-pass GPU p50/p95: floor 0 **1.580/1.699 ms**, floor 2 **1.525/1.649 ms**.
  Mean fragment invocations: **244,246** versus **245,648**. Coupled simulation p50/p95:
  **1.891/2.050 ms** versus **1.867/2.007 ms**. Each timing distribution has 600 observations.
  These are two sequential observations, **not evidence the policy speeds up rendering**, a
  whole-frame sum, or a certified budget: clocks, phase/work distribution and background load
  can vary. The trail-heavy probe also does not isolate worst-case tiny-sprite fill or the
  cost of F4D's offscreen culling bypass. Test those on target hardware before choosing a default.

Reproduce either run by setting `--sprite-min-pixels` to 0 or 2:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe event-trail-volley --camera wide --backend gpu --history playback-only --hdr --exposure 0 --sprite-min-pixels 2 --gpu-bench target/fireworks-f4/sampling-volley-2-bench.json
```

Verification: native temporal regression, viewer suite **52 passed, one ignored**, strict
renderer/viewer all-target Clippy and formatting/diff checks pass. No new runtime passes,
simulation/replay state or production defaults are introduced. F4 remains in progress for
reference-footage/artistic acceptance, narrow trails, offscreen/fill stress and finale budgets.

### F4F implemented — dense sprite fill and offscreen submission probes (2026-10-02)

- Added viewer probes `f4-sprite-fill` and `f4-sprite-offscreen`: one analytic emitter and
  one additive sprite draw, 65,536 stationary particles with constant size/color/opacity,
  no trails/events or replay storage. At the 960×540 reference physical viewport, quarter-pixel
  quads occupy a 16×16-pixel patch. A 30-second lifetime keeps the cohort unchanged throughout
  the benchmark's measured window. IDs and seed are fixed; close/audience/wide presets each
  calibrate the same projected footprint rather than accidentally comparing different sizes.
- The offscreen pair moves the effect through **host placement**, keeping its local bounds
  near zero, with its patch centered at NDC x=2. Baking the offset into an emitter would make
  the origin-centered conservative AABB intersect the frustum and invalidate the ordinary
  culling baseline. Tests use the actual GPU dynamics bounds and Bevy frustum intersection
  to gate this distinction for all three cameras. CPU samples gate stationarity and full
  cohort count; CLI preparation tests cover legacy and migrated semantic materials.
- Benchmark presentation metadata adds `legacy_material_migration` and optional `raster_probe`
  calibration (count, nominal quad/patch pixels, reference physical viewport, center NDC x).
  Calibration is not a claim about a resized/HiDPI window: compare observed physical sizes.
  Viewport-smoke mode is rejected for these probes because it replaces the calibrated camera.
- Extended the native raster test for both legacy and constant-alpha semantic shader paths:
  untreated quarter-pixel quads just outside each of the four viewport edges are invisible;
  the expanded footprints correctly cross those edges at 2/4 pixels with bounded coverage.
  Wholly offscreen rotated quads remain fully clipped even at the 8-pixel policy maximum.

Six sequential native-GPU development-build runs on **RTX 4070 SUPER / Vulkan**, 960×540,
high tier, wide camera, migrated semantic material, fixed seed `0xf1e0000000000001`, HDR,
Tony, exposure 0, bloom 0.15, fast transparency and playback-only history:
120 warm-up + 600 measured viewer frames per run. All reports observe native GPU only,
65,536 live particles and estimated effect-buffer memory **4,194,328 bytes**. No event/trail
measurements are claimed for this analytic sprite-only workload.

| Probe | Pixel floor | Transparent-pass p50 / p95 (ms) | Vertex invocations/frame | Fragment invocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Dense patch | 0 | 0.540 / 0.542 | 262,144 | 16,543 |
| Dense patch | 2 | 0.856 / 0.863 | 262,144 | 577,752 |
| Dense patch | 4 | 1.419 / 1.617 | 262,144 | 1,692,139 |
| Offscreen | 0 | unavailable (no transparent-pass samples) | unavailable | unavailable |
| Offscreen | 2 | 0.168 / 0.170 | 262,144 | 0 |
| Offscreen | 4 | 0.170 / 0.171 | 262,144 | 0 |

Each available pass/counter has 600 fresh observations; vertex/fragment counts are constant
in this stationary scene. Offscreen treated draws output zero clipped primitives, while
ordinary frustum culling removes the untreated draw; absence of a pass is **not** encoded
as a measured zero duration. HDR captures at frames 120/360 are byte-identical across all
three offscreen policies. Simulation p95 ranges 0.717–0.724 ms, separately measured; do not
sum it with transparent-pass percentiles or call this a complete-frame/post-process budget.

These measurements expose the real quality/cost tradeoff: minimum-footprint treatment avoids
dropout but adds substantial fill under concentrated overlap, and the current safety bypass
still spends vertex work on invisible effects. They are sequential observations, not a
hardware-independent budget or proof of final-show performance. Keep the default disabled;
target hardware/scene tests and conservative projection-aware culling are follow-ups before
making a production-wide sampling policy. No simulation, shaders or runtime culling defaults
change in this slice.

Reproduce each combination with one of the two probe names and pixel floors 0/2/4:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f4-sprite-fill --camera wide --semantic-materials --backend gpu --history playback-only --hdr --exposure 0 --sprite-min-pixels 2 --gpu-bench target/fireworks-f4/f4-sprite-fill-2-bench.json
```

Verification: viewer suite **55 passed, one ignored**, native projection/static/temporal/edge
raster regression and strict renderer/viewer all-target Clippy pass. F4 remains in progress:
artistic reference comparison, narrow-trail sampling and full finale/target-tier budgets remain
separate acceptance gates; the new sprite stress probes do not substitute for them.

### F4G implemented — conservative sampled-sprite queue culling (2026-10-02)

- Both 2D and 3D native render queues can now reject fully offscreen, minimum-footprint
  additive Sprite draws before pipeline specialization/submission. Main-world visibility
  stays permissive: the ordinary unpadded AABB must not discard expanded edge footprints.
  Camera data is prepared once per view; no per-particle CPU scan, GPU dispatch or particle
  readback is added. Simulation continues when drawing is skipped.
- Bounds refresh from current analytic dynamics and the exact propagated draw transform.
  Particle-position bounds are separate from billboard geometry: an outward-rounded sphere
  encloses every rotated quad corner using maximum authored size and emitter maximum scale.
  The projected side planes also include the pixel floor's full corner radius, physical
  main-pass resolution/aspect ratio and a one-pixel raster/rounding allowance. Near/far
  rejection is intentionally left to raster clipping; bounds crossing the eye plane fail open.
- Eligibility is deliberately narrow: effects with stateful/event/attachment simulation
  records and materials with vertex offsets retain the fallback. So do trail host-motion
  histories, nonuniform/sheared/singular effect transforms, scaled/sheared cameras, temporal
  jitter, nonstandard projection Jacobians, disagreeing custom clip overrides and invalid or
  overflowing bounds. Standard off-center perspective/orthographic cameras and independent
  viewport origins/resolutions are supported. The host sampling default remains disabled.
- Six CPU regressions cover all four expanded viewport edges, fully offscreen rejection,
  independent views, main-pass resolution overrides, rotated/mirrored uniform transforms,
  invalid/unbounded cases and a shader-geometry corner oracle over pixel aspect, size, scale
  and rotation. The existing reversible visibility-policy test and required native raster
  regression (legacy + semantic paths, static/temporal/edge cases) pass. A retained-phase
  transition regression verifies draw removal and re-entry in both 2D and 3D: skipping a queue
  insertion alone would leave the previous frame's entry alive. Viewer tests remain
  **55 passed, one ignored**; strict renderer/viewer all-target Clippy passes.

Four sequential native-GPU development-build runs repeat the F4F setup on **RTX 4070 SUPER /
Vulkan**, 960×540, high tier, wide camera, migrated semantic material, fixed seed, HDR/Tony,
exposure 0, bloom 0.15, fast transparency and playback-only history (120 warm-up + 600 measured
frames). Reports are `target/fireworks-f4/f4g-sprite-{fill,offscreen}-{2,4}-bench.json`.

| Probe | Pixel floor | Transparent-pass p50 / p95 (ms) | Vertex invocations/frame | Fragment invocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Dense patch | 2 | 0.858 / 0.863 | 262,144 | 577,752 |
| Dense patch | 4 | 1.516 / 1.623 | 262,144 | 1,692,139 |
| Offscreen | 2 | unavailable (no transparent-pass samples) | unavailable | unavailable |
| Offscreen | 4 | unavailable (no transparent-pass samples) | unavailable | unavailable |

The treated offscreen queue now has no transparent-pass observations, matching F4F's
untreated culling baseline instead of submitting 262,144 invisible vertices. This is **not a
measured zero-duration pass**. In-view vertex/fragment counts match F4F exactly for all 600
observations; concentrated fill remains expensive, and sequential timings are not a controlled
whole-frame performance guarantee. All four runs keep **65,536 live particles** and estimated
effect-buffer memory **4,194,328 bytes**. Offscreen simulation p95 is 0.743/0.765 ms, independently
measured; do not sum percentiles. Offscreen HDR captures at frames 120/360 are byte-identical
to the F4F background under both policies (`target/fireworks-f4/f4g-offscreen-{2,4}-capture`).

F4 remains in progress: this resolves the analytic offscreen-submission probe, not history-aware
stateful culling, dense visible fill, narrow-trail sampling, artistic reference matching or full
finale/target-tier budgets. There is no new host API and no change to authored assets, particle
state, shaders, replay policy or production-wide sampling defaults.

### F4H implemented — opt-in minimum-width additive trails (2026-10-02)

- Added independent host resource `TrailRasterSampling { minimum_pixels }`, re-exported by
  `aestra-bevy`. Default 0; finite 0..8 physical main-pass pixels. Only native GPU additive
  **Trail** draws qualify: ribbons, other blends, sprites/meshes/flipbooks and CPU/readback
  presentation are unchanged. This is raster treatment, not Time/Distance/Adaptive history
  sampling, pool LOD, replay storage or a new authored trail width.
- Shared legacy/semantic trail vertices measure each point's faded projected width through
  the unjittered main-pass camera, widening only undersampled geometry. Final coverage is
  attenuated by inverse width on strip bodies and inverse area on circular caps. Existing
  endpoint frames keep shared joins attached. Original particle color/opacity/size, width,
  age fading, UV distance, history, event demand and simulation remain unchanged. Zero-width,
  expired and already-resolved geometry do not acquire new treatment/energy.
- Reuses the Trail-only `frames[63].w` presentation lane; renderer/storage binding sizes are
  unchanged. Portable artifact and dynamic builders explicitly initialize it to **0**, not
  the flipbook UV default of 1. Bevy uploads policy changes with current render inputs without
  rebuilding/restarting the simulation. Host control is independent from `SpriteSampling`.
- Until per-view pixel-padded trail bounds are available, enabled trails bypass the existing
  unpadded GPU spatial cull. CPU visibility stays conservative, raster clipping remains and
  valid empty histories can still reject. Native culling regressions cover this fallback,
  empty histories, excluded blends and restoration of normal culling at floor 0. Offscreen
  submissions and increased visible fill are costs, not an optimization/free quality gain.
- Viewer adds `--trail-min-pixels 0..8`, rejects treatment for CPU/readback requests and records
  `trail_minimum_pixels` in benchmark/capture response metadata. Existing schema-1 consumers
  can ignore the additive field; absence in older reports means default-disabled treatment.
  CLI tests prove unchanged compiled assets, seeds, history and photographic settings.

Required native **RTX 4070 SUPER / Vulkan** raster tests use 16 independent, joined four-pixel
trail bodies crossing 32 pixel phases: **512 samples per row/path**, untextured procedural
mask, feather 0.2, linear coverage before HDR/bloom. Legacy additive and constant-alpha semantic
paths produce the same values (semantic fading/attenuation does not depend on an authored
ParticleOpacity node). Both body-only and round-cap variants pass; selected body results:

| Age fade | Pixel floor | Dropouts / 512 | Mean summed alpha | Relative temporal standard deviation |
| ---: | ---: | ---: | ---: | ---: |
| 1 | 0 | 400 | 0.875000 | 1.8898 |
| 1 | 2 | 0 | 0.900015 | 0.1831 |
| 1 | 4 | 0 | 0.899995 | 0.1029 |
| 0.5 | 0 | 464 | 0.187500 | 3.1091 |
| 0.5 | 2 | 0 | 0.225004 | 0.1831 |
| 0.5 | 4 | 0 | 0.224999 | 0.1029 |

The body integral is `4 × 0.25 × fade² × (1 - feather/2)`, giving 0.9/0.225. Cap variants
add their original small area rather than a bright widened halo (mean 0.944469/0.943936 at
fade 1 for floors 2/4). Tests also cover angled trails, expanded viewport edges, wholly clipped
offscreen geometry, zero/expired widths, byte-identical resolved widths and unchanged alpha
blends. This bounded probe is not analytic integration of arbitrary textured masks, a guarantee
for sharply tapering/depth-varying joins or custom cameras, or a full-show quality/budget gate.

Wide-camera HDR willow captures at frames **80/150/300/540** compare floors 0/2 with identical
seed, playback-only history, sprite floor 2, exposure 0, Tony and bloom 0.15. Late trails are
visibly continuous rather than broken streaks. Artifacts/reports:
`target/fireworks-f4/f4h-willow-trail-{0,2}`. Reproduce the treated capture:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f3-willow --camera wide --backend gpu --history playback-only --hdr --exposure 0 --sprite-min-pixels 2 --trail-min-pixels 2 --sample-frames 80,150,300,540 --capture target/fireworks-f4/f4h-willow-trail-2
```

Verification: native sprite/trail raster and trail-culling regressions, policy tests, portable
WGSL/SPIR-V/HLSL shader/Trail contracts, reviewed shader snapshots, viewer suite **56 passed,
one ignored**, and strict renderer/viewer all-target Clippy pass. F4 remains in progress:
all-shell reference matching, trail fill/offscreen budgets and production finale/target tiers
must still be evaluated before choosing any sampling default. No shipping default is enabled.

### F4I implemented — dense trail raster-cost and playback-work probes (2026-10-02)

- Added `f4-trail-fill` / `f4-trail-offscreen`: a single analytic emitter, **8,192 live
  particles and owner slots**, one additive Trail draw, no sprite heads/events, eight-point
  pools, flat caps, Time sampling at 1/60 second and 0.08-second tails. Particle size is 1;
  authored head width projects to 0.25 pixels at the reference 960×540 viewport. A camera-aligned
  16×16-pixel seed patch moves at 15 pixels/second. The short history fits with its boundary
  anchor/head rather than silently truncating a one-second tail into eight points.
- Each pair shares IDs, seed, material and simulation. Only its name and host placement differ;
  offscreen placement starts at NDC x=2 and moves farther right. CPU projection/trajectory tests
  cover all three camera presets, full live counts, constant width/color and the measured 2–12
  second window. CLI compilation covers legacy and migrated semantic paths. These concentrated
  short-strip fixtures are not curved/rounded hero trails, finale content or a quality tier.
- Raster **benchmarks only**, including the older sprite probes, now use a recorded 1/60-second
  simulation step per viewer frame. Normal playback, explicit-frame captures and non-raster
  live-throughput benchmarks retain their existing clocks. An initial wall-clock attempt reached
  retirement during measurement and was rejected; only the final `f4i-f4-trail-*-bench.json`
  reports below are acceptance evidence. Fixed-step raster cost is not proof of real-time catch-up.
- Reports add optional `raster_probe.trail` calibration and `fixed_simulation_step_seconds`.
  Trail reports leave `nominal_quad_pixels` null rather than claiming a sprite footprint.
  `min_live_particles` / `min_occupied_trails` complement peaks, preserving unavailable values
  as null; a peak alone can hide a cohort retiring later. Counts remain asynchronous host
  observations, not frame-aligned certificates. Fresh timestamp and pipeline-statistic sample
  counts must be reported, not assumed equal to the 600 measured viewer frames.

Six sequential native-GPU development-build runs on **RTX 4070 SUPER / Vulkan**, 960×540,
high tier, wide camera, migrated semantic material, seed `0xf1e0000000000001`, HDR, Tony,
exposure 0, bloom 0.15, fast transparency, sprite floor 0 and playback-only history. Each run
has 120 warm-up + 600 measured viewer frames. All six observe native GPU only, minimum/peak
live particles and occupied owners **8,192**, zero retired owners/evictions/truncation, and
estimated effect-buffer memory **5,445,936 bytes**. Named time/counter paths cover the trail-only
transparent pass, not simulation, post-processing, total frame time or the whole example host.

| Probe | Trail pixel floor | Transparent-pass p50 / p95 (ms) | Vertex invocations p50 / p95 | Fragment invocations p50 | Fresh time / counter observations |
| --- | ---: | ---: | ---: | ---: | ---: |
| Dense patch | 0 | 0.113 / 0.193 | 163,840 / 196,608 | 4,910 | 326 / 446 |
| Dense patch | 2 | 0.133 / 0.222 | 163,840 / 196,608 | 73,570 | 326 / 442 |
| Dense patch | 4 | 0.139 / 0.220 | 163,840 / 196,608 | 147,126 | 324 / 439 |
| Offscreen | 0 | 0.002 / 0.003 | 0 / 0 | 0 | 317 / 437 |
| Offscreen | 2 | 0.110 / 0.202 | 163,840 / 196,608 | 0 | 313 / 428 |
| Offscreen | 4 | 0.113 / 0.214 | 163,840 / 196,608 | 0 | 324 / 439 |

History expiry/head observations vary submitted segment counts; these are distributions, not
identical per-frame work claims. The offscreen untreated path still has a measured pass with a
zero-instance indirect draw, unlike F4G's absent sprite pass. Treated offscreen paths produce
zero clipped primitives/fragments but pay vertex cost through the safe spatial-culling bypass.
The roughly 15×/30× median fragment increase in this overlap patch exposes the fill tradeoff;
single sequential timings/clock variance do not establish scaling laws or imply floor 4 is
cheaper than floor 2 from one p95. No sampling default or runtime culling changes in F4I.

**Playback blocker exposed, not optimized away:** paired simulation frames observe the normal
one-history-observation workload and also **240-observation / 455,760-history-workgroup batches**
during the live playback-only run. In the dense floor-0 report, normal one-observation work has
p95 **0.739 ms**, while four 240-observation frames cost **61.915–62.030 ms** and report a processed
time of 3.9833333 seconds. Aggregate simulation p99 across the six reports is **61.915–62.167 ms**.
This is explicit extra history work, not a raster-cost improvement or merely discarded timestamp
noise. In this analytic trail path, the timing report's `requested_time` is populated from the
last processed observation; it is not proof of reaching the viewer's eventual target. Do not sum
pass percentiles, claim smooth production playback or hide rebuilds with an average.
The next runtime investigation should trace why this static-placement, forward-only fixture
reconstructs history and prevent unnecessary live reconstruction without breaking genuine
restart/seek/context-edit correctness. Pixel-padded trail culling remains a separate opportunity.
**Follow-up:** F4J below resolves the avoidable clock-driven invalidation in these probes;
the F4I measurements above remain the pre-fix evidence, not current performance claims.

Captures at frames 120/360 verify the in-view overlap patch. Offscreen floors 0/4 are byte-identical
at both frames (`target/fireworks-f4/f4i-{fill-2,offscreen-0,offscreen-4}-capture`); the shader's
clipped fallback adds no visible geometry. Reproduce all six measurements using the two probe
names and floors 0/2/4:

```powershell
cargo run --locked -p aestra-viewer -- --fireworks-f0 --fireworks-f0-probe f4-trail-fill --camera wide --semantic-materials --backend gpu --history playback-only --hdr --exposure 0 --trail-min-pixels 2 --gpu-bench target/fireworks-f4/f4i-f4-trail-fill-2-bench.json
```

Verification: viewer suite **58 passed, one ignored**, history-budget/projection/trajectory,
fixed-clock/metadata and measured-minimum telemetry regressions, strict viewer all-target
Clippy, and native captures/measurements above. F4 remains **in progress**: the new probes make
trail cost and the live history-work spike observable; they do not close playback/finale budgets,
all-shell photographic reference review, textured/tapered/cap costs or lower-device tiers.

### F4J implemented — clock-authoritative live analytic trail playback (2026-10-02)

- Reproduced the generic player bug with a failing regression at **forward frame 3**:
  analytic playback first added an f32 tick delta to instance time, then corrected it to
  the exact frame-clock time. A one-ULP downward correction called `set_playback_time`,
  which correctly recognized a backward discontinuity and bumped the history epoch.
  Repeated false epochs made the renderer reconstruct trails during ordinary live playback.
- Added engine-neutral `EffectInstance::advance_clock_with_choreography_events` and wired
  the Bevy analytic player to it. Consecutive forward clock snapshots now supply time once;
  no intermediate f32 integration/downward correction occurs. Single-effect and project
  cue enumeration share clock/cycle windows, including fractional-duration restart loops
  rounded to final frames, authored continuous-loop periods and end-before-start ordering.
  Stateful per-tick integration remains unchanged.
- No epsilon suppresses actual backward seeks: even a one-ULP backward external time change
  still invalidates history. Restart loops invalidate once per advance crossing a loop;
  continuous forward loops do not. Explicit seeks/restarts, seed/context edits, bounded GPU
  reconstruction and the PlaybackOnly/ReplayEnabled checkpoint distinction are preserved.
  No trail pool, sampling, material, raster policy, culling or shipping default changes.

Three sequential native-GPU development-build reruns use the **same F4I setup**: RTX 4070
SUPER / Vulkan, 960×540, high tier, wide camera, migrated semantic material, fixed seed,
HDR/Tony/exposure 0/bloom 0.15, playback-only history, 1/60 step, 120 warm-up + 600 measured
viewer frames. Each delivers **600 fresh paired simulation observations**, all in the single
work group `observations=1, workgroups=1899`: **zero 240-observation reconstruction batches**.
All retain minimum/peak live particles and occupied trail owners **8,192**, zero retired owners,
evictions/truncation, and estimated effect-buffer memory **5,445,936 bytes**.

| Probe | Trail floor | F4I simulation p99 (ms) | F4J simulation p50 / p95 / p99 / max (ms) |
| --- | ---: | ---: | ---: |
| Dense patch | 0 | 61.915 | 1.513 / 1.651 / 1.660 / 1.684 |
| Dense patch | 2 | 62.022 | 1.606 / 1.696 / 1.705 / 1.719 |
| Offscreen | 2 | 62.161 | 1.430 / 1.701 / 1.713 / 1.733 |

Reports: `target/fireworks-f4/f4j-f4-trail-{fill-0,fill-2,offscreen-2}-bench.json`.
This is evidence that the extra history work and its roughly 62 ms tail latency disappear,
**not** an optimization of one-observation dispatches: current normal-work medians/p95 are
higher than F4I, and fresh delivery counts/execution cadence differ. No GPU-clock explanation
is claimed without measurement. Transparent-pass p95 is 0.497/0.536/0.446 ms respectively;
those values are not raster improvements or whole-frame budgets. The fixed-step probes still
do not certify real-time catch-up, production finale throughput or lower-device tiers.

Exact-frame floor-2 dense captures at **120/360** are byte-identical to F4I, preserving seek
output (`target/fireworks-f4/f4j-fill-2-capture`). Verification: runtime **70 unit + one
integration tests**, Bevy **40 tests**, renderer **five trail-reconstruction tests**, required
native GPU trail history/seek conformance, viewer **58 passed, one ignored**, and strict
all-target Clippy for runtime/Bevy/renderer/viewer. New regressions cover 1,800 forward ticks
under both history policies, Once/continuous/restart modes, fractional loops, speed/batched
ticks, single/project cues, silent seeks (including the inclusive final frame), restart
re-emission, real one-ULP backward jumps,
seed invalidation and bounded genuine trail reconstruction. F4 remains **in progress**;
Pixel-padded trail culling follows in F4K below; production/reference/tier gates remain open.

### F4K implemented — pixel-aware whole-history trail culling (2026-10-02)

- Replaced F4H's unconditional sampled additive-trail spatial bypass with a conservative
  per-view radius enclosing the widened strip and circular caps. The GPU uses the current
  whole-history position AABB and maximum recorded size, including retired tails, rather
  than the current head alone. Authored width is retained; the pixel floor is bounded using
  the smallest physical-pixel projection axis and maximum positive clip W over the history.
  Frustum planes test the point AABB plus a width/cap sphere; rotated cameras no longer pay
  the unnecessarily loose world-axis cube padding.
- Reused the sprite camera contract: standard rigid perspective/orthographic cameras,
  physical main-pass resolution (including resolution overrides), and consistent unjittered
  projection. Unknown/stale/non-finite bounds, unsupported camera scale/shear/projection,
  invalid pixel coefficients and eye-plane-crossing sampled histories fail open. Jittered
  views and custom vertex displacement keep the existing no-cull fallback. Empty-history
  rejection and per-view indirect counts remain; no CPU readback or additional binding.
- The 80-byte `GpuTrailCullParams` ABI reuses its former padding word as
  `pixel_radius_per_clip_w`. Other hosts can supply zero for the previous sampled spatial
  fallback, or the documented conservative camera coefficient for supported views.
  CPU visibility remains permissive. This is whole-draw raster rejection, **not** simulation
  culling, per-segment compaction, trail-pool reduction or a new shipping pixel-floor default.

Sequential native-GPU development-build runs use the same F4I/F4J setup: RTX 4070 SUPER /
Vulkan, 960×540, high tier, wide camera, migrated semantic material, fixed seed,
HDR/Tony/exposure 0/bloom 0.15, playback-only history, 1/60 step, 120 warm-up + 600 measured
frames. All three deliver **600 fresh raster and paired simulation observations**, each
simulation observation doing 1,899 workgroups with no 240-observation reconstruction batches.
Minimum/peak live particles and occupied owners stay **8,192**, retired owners/evictions/
truncation stay zero, and estimated effect-buffer memory remains **5,445,936 bytes**.

| Probe | Trail floor | Transparent GPU p50 / p95 (ms) | Vertex invocations min / max | Simulation p50 / p95 / p99 / max (ms) |
| --- | ---: | ---: | ---: | ---: |
| Dense patch | 2 | 0.396 / 0.536 | 163,840 / 196,608 | 1.534 / 1.722 / 1.743 / 1.756 |
| Offscreen | 2 | 0.003 / 0.004 | 0 / 0 | 1.576 / 1.776 / 1.791 / 1.814 |
| Offscreen | 4 | 0.003 / 0.004 | 0 / 0 | 1.629 / 1.789 / 1.806 / 1.844 |

Reports: `target/fireworks-f4/f4k-f4-trail-{fill-2,offscreen-2,offscreen-4}-bench.json`.
F4J's floor-2 offscreen baseline submitted 163,840–196,608 vertices per sample and measured
transparent-pass p50/p95 **0.345/0.446 ms** despite zero fragments. Both F4K offscreen runs
record zero vertex, clipper and fragment invocations throughout; history keeps recording.
The dense probe retains its visible workload and similar raster p95. These are transparent
pass timings, **not** separately measured culling cost, whole-frame budgets or proof of
real-time catch-up/full-show/finale throughput. Simulation is not optimized here; its measured
medians/p95 vary and do not show an improvement over F4J.

Verification:

- Required native GPU decision tests retain expanded footprints at all four viewport edges
  for floors 2/4/8, flat/round caps, translated/rotated perspective and orthographic cameras,
  and differing physical resolutions; distant histories reject. Existing multi-view,
  compacted-count, re-entry, retired-tail and invalidation cases continue to pass.
- Production legacy and semantic additive raster paths produce **byte-identical pixels**
  with culling on/off across all four edges, fade levels, rotated strips, fully offscreen
  histories and cases where only a widened round cap enters the viewport.
- Exact-frame dense floor-2 captures at 120/360 are byte-identical to F4J; offscreen floor-4
  captures at 120/360 are byte-identical to F4I (`target/fireworks-f4/f4k-{fill-2,offscreen-4}-capture`).
- Portable shader contract tests **9 passed** (WGSL/SPIR-V/HLSL translation), renderer camera/
  queue tests **8 passed**, both required native GPU conformance suites passed, viewer
  **58 passed, one ignored**, and strict all-target Clippy for GPU/renderer/Bevy/viewer passed.

F4 remains **in progress**. Next prioritize authored-shell visual/reference acceptance at
close/audience/wide framing, then real-time full-show/finale and target-device tier validation.
Do not choose a shipping pixel floor or declare AAA readiness from these stress fixtures.

### F4L implemented — reproducible all-shell visual review (2026-10-02)

- Added `benchmarks/fireworks/capture-shell-review.ps1`: PowerShell 7, build once, sequential
  native-GPU captures for all four F3 prototypes × close/audience/wide × authored floors 0/0
  versus opt-in floors 2/2. Fixed 960×540/high-tier/seed/60 Hz, playback-only history,
  HDR/Tony/0 stops/bloom 0.15 and stable small-cohort transparency. Eight explicit lifecycle
  frames per case include the final seven-/nine-second cleanup endpoint.
- The runner rejects existing output folders, native-backend/compatibility failures, wrong
  capture metadata, missing images, incomplete observed star peaks, and nonzero/unavailable
  endpoint live/history/eviction/truncation metrics. A machine-readable review manifest
  preserves the plan, source hashes, Git/dirty provenance, adapter, endpoint metrics and PNG
  hashes. `-PlanOnly` is non-mutating; filters deduplicate. Failed runs preserve partial
  evidence. There is **no automatic artistic or golden-reference approval**.
- RTX 4070 SUPER/Vulkan development-build run under
  `target/fireworks-f4/f4l-shell-review` completed **24 cases / 192 frame PNGs**. All cases
  observed 256 main stars; Pistil additionally observed 96 inner stars. Every final report
  measured zero live particles, occupied/retired histories, evictions and truncation.
  These are peak/endpoint checks, not per-frame demand auditing or live performance budgets.
- Inspected all three framings for each shell and selected authored/sampled comparisons.
  Cooling colors, distinct Pistil layers and falling Chrysanthemum/Willow arcs are present;
  narrow untreated trails can look dotted, with better continuity under floor 2 but visibly
  different thickness. The F0 close camera crops the elevated bursts, so it cannot establish
  whole-shell close acceptance. Late red/blue heads, shared circular flash and localized
  smoke still need a chosen photographic target and moving-sequence review.
- Review protocol and findings are in `benchmarks/fireworks/README.md` and
  `benchmarks/fireworks/shell-review-2026-10-02.md`. Plan/parser checks and rejection checks
  for CPU fallback, wrong frames/policies, truncation, unavailable counters and incomplete
  main cohorts pass. No runtime/API, authored effect/material or shipping-default change.

F4 remains **in progress** for artistic acceptance: still captures are neither animation nor
real-footage proof. Fix review-camera framing and select the reference target before approving
images. Production-density/full-show/target-tier gates remain open. F5's bounded generic
two-generation event-chain and host-cue validation is the next runtime milestone; these images
alone do not justify more open-ended culling optimization.

### F4M implemented — reference-driven hero shell (2026-10-02)

- Reviewed the six user-provided photographs. Cannes is the primary pink/gold layer target;
  London informs spark-rich tails/smoke. The references are appearance guides, not calibrated
  radiance, shutter settings or animated acceptance. Originals are not packaged in Aestra.
- Added the separate editable `assets/test/effects/fireworks_reference_hero.aestra.ron` and
  two ordinary project material programs: warm-white age-cooling sprite cores and longitudinal
  tail-alpha taper over existing history-width fade. All four F3 assets and runtime APIs,
  hard limits and sampling defaults remain unchanged. Color gradients and RGB radiance remain
  exposed generic effect parameters; alpha/coverage is independent of radiance gains.
- Seven emitters / ten renderers / 722 physical particle slots. One real rocket death drives
  384 pink outer stars, 96 slower gold stars, 128 loose embers, one compact flash and 48 smoke
  particles. Independent lifetime/speed/drag choices add variation; bounded 64-point histories
  use 60 Hz sampling and 0.7/0.85-second tail lifetimes. This is one generation, **not F5**.
- `f4-reference-hero` uses the saved project asset and independent whole-shell viewer cameras
  without changing F0/F3 framing. Extended the review runner with explicit `-Shell reference-hero`:
  six camera/floor cases, eight lifecycle frames through the eight-second cleanup endpoint,
  expected main/inner/ember/smoke cohort checks and source provenance. Default matrix is unchanged.
- Final RTX 4070 SUPER/Vulkan evidence under `target/fireworks-f4/f4m-reference-hero-framed`
  passed **six cases / 48 frames** with measured expected peaks and zero endpoint live particles,
  occupied/retired histories, evictions and truncation. The first close framing cropped late
  falling tails; the revised final captures contain the visible tails at sampled frames.
  Viewer tests, missing-cohort runner checks and warnings-as-errors Clippy pass.
- Findings/reproduction/remaining gaps: `benchmarks/fireworks/reference-hero-2026-10-02.md`.
  Pink/gold separation and head hierarchy improve, but untreated distant trails remain dotted;
  floor 2 changes width and remains opt-in. Smoke is a more visible **unlit approximation**, not
  burst-driven lighting or trajectory-linked volumetrics. No artistic/golden approval, live
  timing or production-density certification follows from this static run.

F4 remains **in progress**. F5A below implements the bounded generic two-generation
particle-event chain in forward playback without replay history; next is F5B host-cue
validation. F7/F8 retain transient-light/lit-smoke integration. Scene, audio assets/mixing, sky,
reflections and choreography stay host-side; no firework-specific runtime API is introduced.

### HDR material / host convention

- Unlit semantic material `Color` is linear scene RGB and may exceed 1; the generated
  material fragment shader preserves RGB rather than clamping it to display range.
  Alpha/coverage are bounded opacity, separate from radiance. Do not encode emission gain
  by increasing alpha, or tonemap inside the material before additive compositing.
- The existing HDR-specialized render targets support the material and legacy particle
  paths. A separate `Emissive` output is **not required for these unlit fireworks**;
  reconsider it only for a generic lit-material/light-transport requirement.
- Bevy hosts can attach `Hdr`, `Tonemapping`, `ColorGrading`, and optional `Bloom` to the
  effect camera using Bevy's existing API. No firework-specific Aestra runtime concept or
  simulation parameter is introduced. UI/overlay cameras must remain separate.

### Still open before F4 acceptance

- Refine authored radiance/highlight/color preservation against reference footage; the new
  8/4 gains are initial artistic defaults, not physical calibration. F4D adds opt-in sprite
  footprint treatment and F4H adds opt-in trail width treatment; assess remaining textured/
  tapered cases, unsupported-view offscreen submission cost and visible overdraw before
  choosing a production default. F4K removes sampled-trail offscreen raster work for supported
  cameras in the bounded probes, not all workload or camera configurations.
  Trail history LOD and analytic pixel integration remain unchanged.
- Compare all shell types at close/audience/wide framing and assess the night scene,
  smoke and heavy-overlap response. This slice proves the response path, not AAA realism.
- Production budget/finale gates remain open. F4 is **in progress**, not complete.
  F4J resolves F4I's avoidable 240-observation clock-driven history rebuilds in the bounded
  analytic probes; F4K adds safe pixel-aware trail culling. Still measure real-time/full-show
  playback and target-device tiers;
  removing that bug alone does not certify a smooth runtime tier.

### Tasks

- render preview through HDR target where appropriate;
- add exposure control;
- add tonemapping;
- add bloom;
- expose simple preview controls;
- define HDR material conventions;
- review need for explicit `Emissive` material output;
- ensure screenshots/tests have deterministic post-process settings.

### Deliverable

The same chrysanthemum asset visibly changes from "particle graphic" to a plausible luminous night effect without changing particle simulation.

---

## Milestone F5 — Secondary shell behaviors

**Goal:** validate event chains and heavy event load.

Build:

- Crossette.
- Crackle.
- Multi-break shell.
- Strobe shell.

### Optional runtime improvement

Add age/condition event triggers if authoring these reveals real friction.

Do not implement them merely because they sound useful; use the showcase to justify the exact semantics.

### Host cue integration

Expose the selected particle/effect cues through the generic host-facing contract in section 19. Prove a Bevy host can bind launch and burst audio to actual simulation events, including after restart and seek, without double-playing or timing them from a separate script. Keep audio playback, assets and spatial mixing in the example host.

### Deliverable

At least one shell uses two generations of particle-driven spawning.

### F5A implemented — bounded multi-break chain (2026-10-03)

- Ordinary editable asset `assets/test/effects/fireworks_multi_break.aestra.ron`, selected by
  viewer probe `f5-multi-break`. One rocket's `OnDeath` requests 64 main stars, one flash and
  48 smoke particles. Each main star's real `OnDeath` requests eight secondary sparks:
  **512 total**, at the parent death position, with 25% inherited velocity. There is no timed
  fake secondary burst, new trigger, duplicated event-link workaround or fireworks API.
- Six emitters, total particle capacity 690. Secondary speed 3–6, lifetime 0.45–0.85 seconds;
  main lifetimes 1.2–1.6 seconds stagger the breaks. Secondary histories retain 0.35 seconds
  at 60 Hz with 64 points and 512 owners. Hero materials, exposed color/radiance controls and
  unlit smoke are reused. Seven-second once playback covers smoke/history cleanup.
- Production GPU compute conformance covers a simultaneous 64-parent / 512-child death
  wave, translated launch origin, CPU/GPU positions by ordinal, parent death origins,
  velocity inheritance, link-order independence and eventual empty state. A required-GPU
  run passed, advancing forward without checkpoints, seeks or restarts.
- Uninterrupted native renderer evidence: `target/fireworks-f5/f5a-multi-break-live.json`.
  RTX 4070 SUPER/Vulkan, 960×540, high tier, HDR/Tony/exposure 0/bloom 0.15, stable ordering,
  seed `0xf1e0000000000001`, **playback-only**. All four links report demand = accepted
  **64 / 1 / 48 / 512**; omitted/rejected and source overflow are zero. Observed trail
  evictions/truncation are zero. All 600 recorded simulation samples have zero coupled
  checkpoint-capture bytes. F5 bench mode records fixed 60 Hz ticks to cover the full lifecycle
  independent of render speed; interactive mode remains wall-clock driven. This finite shell /
  idle-tail run is admission evidence, **not finale throughput certification**.
- Review runner accepts `-Shell multi-break`. Three authored-floor camera cases / 24 PNGs
  under `target/fireworks-f5/f5a-multi-break-review` pass native-backend, cohort and cleanup
  checks. At frame 420, measured live particles, occupied/retired histories, evictions and
  truncation are zero. Stills are separate from uninterrupted admission proof. Full viewer
  tests (63 passed / three fixture exporters ignored), the full required-GPU stateful
  conformance suite (28 passed, serial) and warnings-as-errors Clippy pass. Reproduction/limitations:
  `benchmarks/fireworks/secondary-shell-2026-10-03.md`.

### F5B implemented — generic particle-driven host cues (2026-10-03)

- The saved multi-break asset exports three ordinary particle routes: rocket `OnSpawn`
  → `launch`, rocket `OnDeath` → `main_break`, main-star `OnDeath` → `secondary_break`.
  `FirstPerTick` carries one representative effect-local position plus source-particle count.
  Host-owned sound identifiers are selected from received messages, not a separate timer.
  No audio files, playback or spatial mixing were added to Aestra.
- Native GPU output records reuse their header padding word for the playback epoch.
  Route high-water marks reject stale epochs, duplicate/out-of-order readbacks and ticks
  through a seek's reconstruction boundary. Checkpoint restores preserve a fresh delivery
  epoch rather than restoring an old one. `AestraOutputEvent.playback_epoch` qualifies native
  particle-route and timeline messages; `None` explicitly leaves legacy/stage outputs
  unqualified. The ring remains bounded to 32 ticks; no lossless gameplay-bus claim.
- `--fireworks-cue-check` consumes the actual Bevy messages on native GPU / playback-only,
  warms at zero, plays the full shell, seeks to frame 160, resumes, then restarts. Initial
  and restarted streams each deliver one launch, one main break and 21 secondary packets
  accounting for all 64 parent deaths; resumed seek delivers 16 packets / 55 remaining deaths.
  Representative positions, counts and ticks reproduce across restart. Reconstructed past
  cues are silent. Root fixture validation does not certify nested spatial-audio routing,
  heavy overlapping output traffic, deliberate readback stalls or legacy output producers.
- Forward admission remains demand = accepted **64 / 1 / 48 / 512**, with zero source
  overflow, omission/rejection, trail eviction/truncation and checkpoint-capture bytes.
  Required-GPU conformance passes (28 serial tests); epoch/ABI/delivery and host-consumer
  regression tests and warnings-as-errors Clippy pass. Evidence and reproduction:
  `benchmarks/fireworks/host-cues-2026-10-03.md`.

### F5C implemented — bounded delayed-carrier crackle (2026-10-03)

- Ordinary editable asset `assets/test/effects/fireworks_crackle.aestra.ron`, viewer probe
  `f5-crackle`. Rocket death feeds 96 main stars; each real main-star death spawns one
  burning carrier with 85% inherited velocity. Carrier lifetime 0.12–0.35 seconds provides
  seeded delay; its real death requests 12 short-lived sparks (1,152 total), with 15%
  inherited velocity. This is three particle-driven spawning generations, without timed
  fake crackle, new triggers or duplicated links to bypass engine limits.
- Main stars live 0.75–1.05 seconds; spark lifetime 0.08–0.20 seconds and bright-to-gold cooling
  make impulsive pops rather than persistent secondary tails. Carriers and sparks are
  sprite-only; main and launch histories retain the existing hero trail policy. Seven emitters,
  capacity 1,458, exposed color/radiance parameters, reused HDR hero materials and unlit smoke.
- Native forward / playback-only evidence admits demand = accepted **96 / 1 / 48 / 96 / 1,152**;
  source overflow, omissions/rejections, trail evictions/truncation and all checkpoint-capture
  bytes are zero. RTX 4070 SUPER/Vulkan, 960×540, high tier, fixed 60 Hz, authored floors.
  This lifecycle/idle-tail run proves admission, not finale throughput.
- Native host check binds `crackle` from carrier `OnDeath`, not a sound timer: initial and
  restarted runs each deliver 27 packets representing 96 pops, ticks 134–164. Seek to frame
  160 resumes five packets / eight remaining pops with no reconstruction playback; payloads
  match the original/restarted streams. Sounds, spatial voices and assets remain host-owned.
- GPU compute regression compares three-generation state and output payloads against CPU at
  intermediate/final ticks, verifies 96 carriers and all 1,152 spark retirements, and eventual
  empty state. Routed births deliberately do not produce `OnSpawn` in the current contract;
  birth admission comes from link counters, not that output route. Full required-GPU suite
  passes (29 serial tests), viewer passes (67 / four exporters ignored), Clippy passes.
- Three camera cases / 24 exact-frame PNGs pass cohort and endpoint checks. Stills show the
  red main burst followed by brief warm dots and smoke; human/moving-footage artistic
  acceptance remains open. Evidence/reproduction: `benchmarks/fireworks/crackle-2026-10-03.md`.

### F5D implemented — bounded four-arm crossette (2026-10-03)

- Ordinary editable asset `assets/test/effects/fireworks_crossette.aestra.ron`, viewer probe
  `f5-crossette`. One rocket death feeds 32 main stars. Each main-star death feeds four
  **distinct directions**, one child per link, not four random spherical samples: constant
  NE/NW/SW/SE directions in effect-local XY. All arms share the real death position and
  30% inherited velocity; equal speed 10, lifetime 0.75 seconds, drag 0.25 and gravity keep
  opposite arms symmetric about their moving center. Main lifetime 0.85–0.95 seconds staggers
  the splits. Four emitters express four genuinely different velocities, not a workaround
  for capacity limits. No new runtime trigger, timer or firework-specific API.
- Nine emitters, seven links, capacity 274, including 128 arm particles and 161 trail slots.
  Arm histories last 0.5 seconds at 60 Hz within 64 points per owner. Reuses HDR materials,
  exposed radiance/color controls and unlit smoke. Fixed effect-local plane follows effect
  placement, **not each parent's heading**; per-parent orientation/roll remains unsupported
  by this fixture, not certified by shared velocity inheritance.
- Native uninterrupted GPU / playback-only admits demand = accepted
  **32 / 1 / 48 / 32 / 32 / 32 / 32**. No source overflow, omissions/rejections, trail eviction
  or truncation; checkpoint-capture bytes remain zero. RTX 4070 SUPER/Vulkan, high tier,
  960×540, manual 60 Hz. Lifecycle plus idle tail is not finale throughput certification.
- `crossette_split` comes from actual main-star deaths, before the four arms; one host cue
  per split, coalesced `FirstPerTick`, not one sound per arm. Initial/restarted streams each
  account for all 32 parents in six packets at ticks 132–137. Seek to frame 133 reconstructs
  silently through tick 132; five packets / 29 remaining parents resume and match the
  original stream. Actual sounds, placement and voice pooling remain host-owned.
- Production compute conformance compares CPU/GPU live positions at intermediate ticks,
  checks births at real parent positions, four-way geometry, opposite-arm symmetry,
  link-order independence, inherited-motion contribution and all 128 eventual retirements.
  Required-GPU suite: 30 passed; viewer: 69 passed / five exporters ignored; Clippy passes.
  Three camera cases / 24 PNGs pass full-cohort and endpoint cleanup checks. Stills remain
  dim/sparse at audience distance; artistic/moving-footage acceptance remains open.
  Evidence/reproduction: `benchmarks/fireworks/crossette-2026-10-03.md`.

### F5E implemented — bounded independently phased strobe (2026-10-03)

- Ordinary editable `assets/test/effects/fireworks_strobe.aestra.ron`, viewer probe
  `f5-strobe`. Rocket death requests 192 sprite-only main stars, one flash and 48 smoke
  particles. Main lifetime is fixed at three seconds; exposed **18 cycles per life** and
  **0.18 duty** give six flashes/second, with seeded independent phase offsets. Generic
  `periodic_gate.aestra.material-function.ron` gates both hot-core RGB and alpha to exact
  zero outside the on interval. No main-star trails bridge the off interval, no mutable
  host timer, no new simulation trigger or fireworks API. Lifetime edits change Hertz;
  normalized-age timing is deliberate, not a new absolute-particle-age input.
- The existing declared `ParticleRandom` material input now reaches native particle
  presentation as a flat varying. Its hash uses render seed, emitter index and stable
  spawn ordinal, not physical/compaction slot. It is a separate presentation stream, not
  a promise to match simulation RNG. Sprite/mesh use the particle, trails their owner,
  ribbons the segment's first endpoint. Particle storage ABI and bind groups are unchanged;
  unreachable random reads consume no new varying. Runtime rendering now lowers already
  compiler-expanded custom calls, instead of rejecting them as authoring nodes. Viewer
  legacy-material migration retains the project's function library.
- Five emitters, total capacity 306, seven-second once playback. Normal effect-bound
  radiance, cycles and duty controls remain live material values. Smoke remains unlit.
  Strobe flashes are appearance changes, **not** particle transitions or sound outputs;
  sound playback/mixing remains host-owned. The periodic function is reusable outside
  fireworks and has no per-frame CPU clock or material rebuild.
- Required native raster test independently verifies the identity hash, exact zero
  off-cell RGB/alpha, mixed phases, every star flashing, duty endpoints 0/1, seed/emitter/
  ordinal changes, physical-slot reorder and repeated/out-of-order ages. Portable shaders
  validate SPIR-V/HLSL and single/MSAA variants; native pipeline linkage covers Random.
  Viewer: 71 passed / six exporters ignored; full aestra-gpu suite and compiler function
  contracts pass. Native sampling/pipeline suites pass; warnings-as-errors Clippy passes.
- Native uninterrupted GPU / playback-only admits demand = accepted **192 / 1 / 48**,
  zero source overflow, omissions/rejections, evictions/truncation and checkpoint-capture
  bytes. RTX 4070 SUPER/Vulkan, high tier, 960×540, manual 60 Hz, authored floors. Three
  camera cases / 24 PNGs pass 192-star cohort and frame-420 cleanup checks. Consecutive
  frames 110/111/112 supplement lifecycle stills, not moving-footage acceptance. Stills
  are sparse/dim at audience distance. Evidence: `benchmarks/fireworks/strobe-2026-10-03.md`.

### F5F implemented — explicit tier profiles and overlapping playback (2026-10-03)

- The real gap was unscaled death-link fan-out. Optional portable
  `EffectAsset::particle_budgets` profiles specify exact emitter capacities, event-link counts
  and trail-owner capacities by stable ID. Existing `EffectCompiler::with_tier` selects by
  name on a copy, before migrations/lowering/resource checks; resolved dependencies use their
  own matching profile. Missing profiles preserve legacy counts; high is authored. No implicit
  multiplication, extra links, raised hard ceilings or firework-specific runtime API.
- Core validation rejects missing/wrong-kind targets, zero/increased values and invalid/high
  names. Flat/v4 round-trip is covered. `SetParticleBudgets` is transactional/diff-visible;
  deleting emitters/events/renderers prunes stale entries and undo restores profiles exactly,
  including combined edits/deletions. Dedicated editor profile UI is not implemented.
- All four F5 singles now ship medium/low profiles. Main/smoke counts halve/quarter;
  multi-break secondary fan-out is 8/6/4 and crackle 12/8/4. Count-1 carriers and all four
  crossette arms survive; strobe timing/material controls, lifetime/motion and output routes
  stay unchanged. Profiles do not scale emission modules or live host input counts. Hosts
  still own admission; a smaller pool alone can reject births. Retired tails need storage.
- The saved nine-second secondary volley uses four real rockets with seeded staggered deaths
  in one source and shared target/history pools. Compiled high/medium/low capacity is
  **2,760 / 1,256 / 632**. This is not full Test B or a finale.
- RTX 4070 SUPER/Vulkan, 960×540, fast transparency, semantic materials, manual 60 Hz,
  playback-only: all links admit main/flash/smoke/secondary **256/4/192/2,048** at high,
  **128/4/96/768** at medium and **64/4/48/256** at low. All overflow/omission/rejection,
  trail eviction/truncation and checkpoint-capture bytes are measured zero. Estimated
  effect buffers **10,862,400 / 4,224,820 / 1,532,980 bytes**; not total VRAM. A read-only
  validator gates future reports on work, not speed. Lifecycle matrix: nine cases/72 PNGs,
  expected cohorts/four arms and full endpoint cleanup; visual approval remains open.
- The native single-shell host checker reads the compiled tier count rather than high-only
  constants. Low multi-break/crackle/crossette cue checks pass at 16/24/8 secondary parents,
  including complete remaining live cues after seek, suppressed reconstruction, restart and
  distinct epochs. No audio assets/playback or nested spatial-cue acceptance is implied.
- Core/compiler/authoring/viewer tests pass, including legacy/high artifact equality and
  tier-before-resource-check contracts; native stateful conformance 30 passed. Workspace
  check and warnings-as-errors Clippy pass. Evidence/reproduction:
  `benchmarks/fireworks/tier-volley-2026-10-03.md`.

F5's bounded technical fixtures and tier mechanisms are implemented, but **artistic and
production-density acceptance remain open**. Low is sparse/dim at audience distance;
mechanism tests do not replace reference-footage review. **F6A below implements a bounded
26-second show built from reusable EffectClips**, with multi-site placement/seeds and tier/admission
telemetry. Parent-oriented crossettes, nested spatial audio, lit/persistent smoke, heavy finale
load and target-hardware gates remain open. No full-show readiness claim follows from F5F.

---

## Milestone F6 — Full 20–30 second show

**Goal:** validate Aestra as a VFX choreography tool, not just a particle emitter editor.

### Show composition

Use reusable shell assets through `EffectClip`.

Include:

- multiple launch sites;
- single shells;
- paired shells;
- fan/volley sections;
- alternating colors;
- layered heights;
- finale.

### Editor UX review

During actual authoring, document friction around:

- locating assets;
- adding/reusing clips;
- parameter overrides;
- seed management;
- moving many clips;
- grouping;
- timeline zoom/navigation;
- duplicate/pattern operations;
- multi-selection;
- preview camera workflow.

The show should feed requirements back into Aestra's timeline UX.

### F6A implemented — bounded reusable composition (2026-10-03)

- Saved v4 `assets/test/effects/fireworks_show.aestra.ron`: 26 seconds, 13 clips reusing
  four F5 shell assets, no local emitters or host-timed particle births. Single/paired
  launches, three-site fan, alternating exposed star-color gradients, per-clip transforms
  and layered heights, then a restrained closing section. Clips run their full seven-second
  lifecycle; the last ends at 24 seconds with a two-second quiet tail.
- Existing project resolution, compiler tier selection, clip scheduler and
  `EffectPlayer::from_project` suffice. Repeated sources share compiled assets but retain
  separate live particle/trail pools and stable clip-path-derived seeds. Peak active clip
  pools are six; conservative concurrent particle capacities high/medium/low are
  **4,460 / 1,980 / 964**. Neither 13 total shells nor six active clip windows is a claim
  about simultaneously bursting shells, Test B density or a 20–40-shell finale.
- Viewer `f6-show` supports ordinary wall-clock interactive playback and opt-in 30-second
  manual-60-Hz benchmarks (120 warm-up + 1,680 measured frames). Reports retain root/path/
  source/seed/policy identity after transient owners disappear. Per-child profiles and
  event readbacks are collected through the existing public project API; concurrent
  project peaks are observed totals, never sums of unrelated per-owner peaks.
- Native RTX 4070 SUPER/Vulkan, 960×540, fast transparency, semantic materials,
  HDR/exposure 0, playback-only: all 13 clips admit expected high/medium/low totals
  **8,413 / 3,317 / 1,217** linked births. Source overflow, expansion omission, destination
  rejection, trail eviction/truncation and checkpoint-capture bytes are measured zero.
  Final observations show zero live particles/occupied histories in each child and project,
  with zero remaining child entities. Peak concurrent estimated buffers:
  **9,430,016 / 3,835,520 / 1,492,544 bytes**, not full VRAM.
- The initial show exposed a truncated rocket history under 1.2× clip scaling. Reusable
  F5 launch trails now use 0.30 rather than 0.25 world-unit distance spacing, retaining the
  portable 64-point cap, 0.35-second retention and original owner counts. A scale-aware
  fixture regression locks headroom for this show's maximum scale, not arbitrary host
  transforms. Later larger/moving placements still need workload-specific sampling gates.
- Tests cover resolution, artifact compatibility, serialization, independent seeds,
  packed overrides, clock/lifecycle boundaries, exact tier capacities, transient-owner
  reporting and concurrent peaks. Read-only `validate-show.ps1` rejects missing/fallback/
  incomplete/dropped/backward/checkpoint observations. Native high/low lifecycle captures
  are inspection evidence, not approved goldens or moving-footage acceptance.
- The composition was authored as a saved fixture, **not through a completed manual editor
  UX review**. Locating/reusing clips, overrides, grouping, patterns, multi-selection,
  timeline navigation and preview cameras still require the review above. Audience views
  remain sparse/small; art direction, radiance/smoke balance and photographic footprint
  need iteration. No AAA or target-hardware speed/finale certification is implied.

Evidence and reproduction: `benchmarks/fireworks/show-composition-2026-10-03.md`.

### F6B implemented — nested spatial particle host cues (2026-10-03)

- Native particle messages now identify the **root player + stable clip path + root
  playback epoch**, not a transient child entity/private GPU epoch. The child GPU record
  is still validated before routing. Typed `ParticleOutputContext` adds source effect ID,
  inherited seed, root-clock occurrence time and optional world position; existing local
  XYZ payloads and source ticks are unchanged. No fireworks-specific runtime concept or
  audio dependency was added.
- World positions compose full affine clip/ancestor/leaf authored transforms at the
  **event tick**, including nonuniform scale/shear. ECS root placement is sampled at
  **delivery time**, not from recorded pose history. Missing/nonfinite positions remain
  unavailable. Historical moving-ECS-root placement and velocity packets remain open;
  this is not the full lossless/per-particle contract described in section 19.
- Fixed the suppression boundary for surviving children on a forward seek and children
  newly created inside a seek. Source-offset/ancestor preroll is silent; normal live
  advancement preserves epochs/boundaries. Unit coverage includes two levels of ancestry,
  historical motion, independent root placement, source offsets, forward/backward seeks,
  stale epochs, duplicate reads and invalid/missing spatial metadata.
- Added ordinary `launch`/`main_break` routes to the strobe source and its fixture builder.
  Material flashes remain appearance, not one sound event per flash. All **13** show
  clips now export their actual launch and main break, rather than certifying only the
  11 previously audible clips.
- Native RTX 4070 SUPER/Vulkan checks at **high/medium/low**, fixed 60 Hz and
  `PlaybackOnly`, run full forward playback, an 18-second seek and a restart. Every full
  pass receives 13 launches + 13 main breaks, plus respectively **704/352/176** secondary
  particle transitions. Resumed streams receive exactly the original cues strictly after
  the seek boundary (**193/97/49** total transitions, including the remaining main break).
  Restarted streams match original clip/seed/kind/tick/count/local/world position, with
  three distinct root epochs and no duplicates or missing selected cues. Clip presentations
  are gone after the quiet tail. This certifies bounded FirstPerTick delivery on this setup,
  not arbitrary readback stalls or performance/finale scale.

Evidence/reproduction: `benchmarks/fireworks/spatial-host-cues-2026-10-03.md`.

**F7A follows below: generic transient-light intents and a bounded Bevy adapter.**
Audio playback/mixing/assets, skybox and host scene/choreography integration
remain host-owned. Manual editor authoring and artistic acceptance remain separate open
F6 gates; F6 is not marked production/AAA-complete by this API check.

---

## Milestone F7 — VFX scene lighting

**Goal:** connect VFX radiance to the host scene at two useful scales:

1. sparse representative event lights;
2. bounded direct particle lights.

The two paths are complementary. Do not replace the existing representative light contract with
per-particle semantics.

### F7A implemented — representative light contract and bounded host adapter (2026-10-03)

- Engine-neutral `PointLightPulse`/`TransientPointLight`: normalized linear RGB, lumens,
  range/radius, lifetime, finite ordered bounded intensity/range curves; occurrence-time
  sampling independent of particle/trail lifetime. No host entities or firework vocabulary.
- Opt-in `AestraTransientLightPlugin` accepts epoch-qualified `AestraLightOutput` intents.
  Defaults: 16 pooled point lights, 128 intents/frame; portable ceilings: 64/1,024.
  Global host enable, lumen/range clamps and live admission budgets; shadows disabled.
  Saturation drops new pulses with counters, never creates a light for each star.
- World-space proxies avoid double root placement, inherit render layers and reuse entities.
  Pause freezes decay. Delayed delivery samples current pulse age, not peak brightness.
  Expiry, seek/restart epochs, root despawn and disable clear live intents. No future queue,
  seek reconstruction of light history, moving proxy attachment or EachEvent identity claim.
- Viewer `--transient-lights` is an explicit example-host binding, one pulse per real
  `main_break`; high/medium/low host budgets remain bounded.
- Native full-show seek/restart checks and bloom-off receiver captures demonstrate real
  ordinary-mesh diffuse response.

Evidence: `benchmarks/fireworks/transient-lights-2026-10-03.md`.

### F7B1 implemented — persisted representative bindings and automatic realization (2026-10-03)

- `EffectAsset::point_lights` / `PointLightBinding` references a stable `FirstPerTick`
  particle-output route and owns color, lumen/range curves, radius and independent pulse lifetime.
- Optional `LightColorParameter` resolves a gradient parameter at a fixed normalized sample age.
- Source v4 round-trips bindings; compiled artifact v6 persists route/parameter references.
- Compiler parameter discovery retains colors used only by light bindings.
- `AestraTransientLightPlugin` automatically realizes saved bindings through the bounded pool.
- Missing, ambiguous or `EachEvent` routes fail validation; magnitude does not multiply intensity.
- Firework shell assets save generic light bindings; no firework-specific runtime mapper remains.

Evidence: `benchmarks/fireworks/authored-lights-2026-10-03.md`.

### F7B2 — editor controls for representative light bindings

**Goal:** make the already-implemented representative-light model fully authorable.

**Implementation (2026-10-03): editor authoring implemented; manual UI/visual acceptance pending.**

- Effect Properties now exposes Representative Point Lights, with compatible route selection,
  constant linear RGB or a shared gradient parameter and normalized sample age, lumen/range curves,
  radius and independent duration. Binding diagnostics navigate directly to the affected light.
- Curves and gradients reuse the cached automation visualization and Feathers numeric controls.
  Key tables expose exact age/output-unit values, interpolation and key insertion/removal; shared
  gradient edits explicitly warn that all users of the parameter change.
- Add/remove/replace are validated semantic commands with binding-level diffs, stable identities,
  locking and undo/redo. Light-only edits enter document history and save normally.
- Copy/Paste settings buttons and Ctrl+C/Ctrl+V while hovering a light card preserve the destination
  binding/curve IDs and trigger route. Numeric text editing keeps normal text clipboard behavior.
  Missing parameter references are rejected, never rebound by name.
- Only unambiguous, unbound FirstPerTick routes are offered. Existing invalid routes stay visible.
  Deleting a bound route/dependency or changing it to an incompatible aggregation is refused atomically until its light is explicitly
  removed or rebound; no silent retargeting. Hosts retain enable/budget/shadow policy.
- Numeric scrubbing commits once on release, avoiding per-drag-frame recompilation/rebuild.

Automated coverage: authoring/history/source round-trip, invalid dependency edits, route filtering,
copy/paste identity, shared gradient editing, normalized curve output units, observer commits,
localized inspector composition, diagnostic targeting and the five F7B1 shell-light settings.

Remaining manual gate: open a shell source in the editor, author/save/reopen its binding and check
the controls at narrow/wide panel sizes. This slice does not install a scene-light adapter in the
editor's lower-level render viewport, and claims no native viewport illumination or lit-smoke proof;
saved bindings continue to be realized by the existing generic Bevy host adapter (F7B1).

#### Editor tasks

Expose on the Effect/Scene Outputs inspector:

```text
Representative Point Lights
  + Add

  Trigger Route       main_break / FirstPerTick
  Color               constant / gradient parameter + sample age
  Intensity            curve (lumens)
  Range                curve
  Radius               scalar
  Duration             seconds
```

Required UX:

- only compatible `FirstPerTick` routes are selectable;
- invalid/missing route state is visible, never silently repaired to another route;
- curve/gradient editing uses existing shared controls;
- undo/redo and copy/paste work;
- validation diagnostics link back to the binding;
- host/global budget policy stays outside the asset.

#### Acceptance gate

A technical artist can add/remove/edit the shell burst light entirely through the editor and reproduce
the saved F7B1 sources without editing serialized data.

### F7C — generic particle scene-output model

**Implemented (2026-10-03): source/core + compiler + artifact + CPU/reference contract.**

- `Emitter::scene_outputs` is a default-empty, material-free list of typed `SceneOutputInstance`s.
  The first built-in is `aestra.scene_output.particle_point_light`; enabled light-only emitters compile
  without a renderer or dummy material. Existing representative `EffectAsset::point_lights` is unchanged.
- `ParticlePointLightProperties` supports particle RGB, constant linear RGB or a shared gradient
  parameter sampled at particle age; normalized-age intensity/range curves, radius, selection policy,
  priority and explicit named quality limits. `high` is the fallback for an unlisted custom tier;
  zero disables candidate evaluation. These authored limits are not global admission guarantees.
- Compilation retains scene-output-only exposed gradient reads, folds non-exposed gradients, resolves
  the quality cap and stores a separate backend-neutral plan on each compiled emitter region. No host
  event route, material, simulation instruction or checkpoint data is added.
- Authored v4 round-trips the additive list and emitter duplication rekeys owned output/curve IDs while
  retaining parameter references. Semantic diffs report scene-output-only changes at the emitter.
  **Artifact v7** explicitly persists plans and validates their IDs, curve envelopes and gradient slots;
  older artifacts must be recompiled, not silently stripped of lighting behavior.
- `CompiledEffect::particle_light_candidates(sample, parameters)` is an allocation-free, read-only
  reference iterator. Invalid/expired samples, zero caps/lumens and invalid live colors fail closed.
  Position remains in the supplied sample's coordinate space; adapters must transform it exactly once.
  Identity includes emitter, region, output ID and stable spawn ordinal, not input vector order.
  Hosts still add root instance/epoch and child occurrence path when merging multiple presentations.
- Ten end-to-end particle-light contracts plus an authoring diff test pass, including material-free
  source/artifact round-trips, gradient-only live parameters, quality caps, malformed data, duplicate
  IDs, reorder/region identities, 4,096 generic ember candidates, and unchanged compiled simulation,
  particle results, timeline events and history identity during candidate evaluation.
  Core/runtime/compiler/artifact/authoring suites, workspace all-target check and warnings-as-errors
  Clippy pass.

**Boundary:** candidates are not selected lights. The reference iterator intentionally does not apply
top-K or instantiate Bevy lights, and does not append particles to a host event queue. It accepts only
alive `ParticleSample`s (that type has no alive flag). GPU candidate compaction/selection, measured
counters, multi-instance admission and visible native particle lighting belong to F7D/F7E; no AAA
scene-lighting performance or visual-acceptance claim follows from F7C.

Minimal authored API (no material required):

```rust
let mut emitter = Emitter::basic_sprite("Embers", 4.0);
emitter.renderers.clear();
emitter.scene_outputs.push(SceneOutputInstance::particle_point_light(
    ParticlePointLightProperties::new(2_000.0, 12.0),
));
effect.emitters.push(emitter);
// Compile normally, then evaluate candidates from the alive presentation samples.
// A candidate stream is a reference input to selection, not a Bevy light spawn loop.
```

**Goal:** add a material-free particle-backed presentation sink without distorting the current
`RendererInstance` contract.

#### Core architecture

Preferred shape:

```text
Emitter
  renderers: Vec<RendererInstance>
  scene_outputs: Vec<SceneOutputInstance>
```

Initial built-in output:

```text
ParticlePointLight
```

Do not implement this as `RendererProperties::Light` with a dummy material. Current core/compiler/plugin
renderer contracts all assume a material-backed renderer.

#### Suggested model

```text
SceneOutputInstance {
  id,
  output_type,
  enabled,
  properties,
}

ParticlePointLightProperties {
  color_source,
  intensity_curve,
  range_curve,
  radius,
  selection_policy,
  max_lights_by_quality,
  priority,
}
```

Shadows remain backend/quality policy and default off.

#### Compilation

Compile each particle-light output into a backend-neutral plan referencing its source emitter and
resolved parameters/curves. It must not produce host events or affect simulation/checkpoints.

#### Compatibility

- existing `EffectAsset::point_lights` remains unchanged;
- old assets need no migration beyond a default-empty emitter scene-output list;
- artifact format changes only when the compiled particle-output plan is persisted;
- plugins should eventually gain scene-output registration if real extension use cases emerge, but
  do not block the first built-in implementation on a complete plugin ABI redesign.

#### Acceptance gate

CPU/reference presentation can deterministically derive light candidates from `ParticleSample` without
changing particle state or output-event semantics.

### F7D — GPU particle-light candidate generation and top-K selection

**F7D1 implemented (2026-10-03): portable per-output selection prototype, not live playback integration.**

- `aestra-gpu::particle_lights` lowers compiled light plans and live gradient values into a separate
  160-byte plan and variable 32-byte key table. All 32 authored curve keys and larger gradients survive;
  there is no reuse/truncation to the simulation ABI's eight-key curve limit. Invalid live color values
  fail lowering closed; adapters must disable the output rather than keep a stale previous plan.
- Read-only evaluation consumes a contiguous source particle pool from the existing 48-byte ABI,
  excluding dead particles, retired trail records, other emitters and invalid samples. Positions receive
  one affine presentation transform; range/radius already use world units. Sprite alpha/size do not
  scale lumens. No simulation state, particle data, host event stream or checkpoint is written.
- Workgroup-local sorting compacts valid candidates into sorted 64-entry blocks. Hierarchical parallel
  merge/rank passes retain at most K per run, with priority/lumens/stable identity keys and deterministic
  ties, not quadratic all-particle comparisons or atomic append order. Final records are 48 bytes each.
  Global counter atomics are aggregated per workgroup, not contended once per source particle.
- `ParticleLightWorkPlan::for_output` honors the minimum of authored quality and host caps, schedules
  reusable ping-pong buffers, checks u32 addressing/device storage/dispatch bounds and returns no work
  for empty/zero-budget jobs. The source token maps back to the host's full occurrence identity;
  cross-output tokens must preserve canonical identity order, not truncate UUIDs or use emitter indices.
- Counters separately expose requested (alive/matching-emitter), valid candidate, selected and
  budget-dropped counts. Invalid inputs are not reported as budget drops. Adapters must clear counters
  before each job; scratch is fully overwritten and does not need per-frame clearing.
- Three engine-neutral tests validate ABI/shader composition, complete key lowering/live override
  rejection, device bounds and caps. Native GPU conformance matches the F7C CPU reference at block/run
  boundaries through 65,536 slots, including live -> empty -> reordered-live reuse, stable ordinal ties,
  all curve modes, static/live gradients, exact counts and unchanged particle input bytes.
- [Measured evidence](../../benchmarks/fireworks/particle-light-selection-2026-10-03.md) covers three
  sequential Vulkan runs on AMD Radeon integrated graphics. The initial selection-only baseline is
  about 0.074 ms at 4,096 slots; the dense 262,144-slot probe needs 4.42-4.70 ms median (p95 up to
  8.46 ms) and 24 MiB scratch
  to retain 64 lights. This is a resource/performance observation, not a full-show/finale acceptance.

**F7D2A implemented (2026-10-03): global admission and live forward GPU integration.**

- Portable global merge passes consume independently quality-capped sorted runs, enforce the host's
  global cap across outputs/instances and aggregate requested/candidate/selected/dropped counters.
  Priority, lumens, canonical occurrence token and spawn ordinal determine ordering; no append order
  or CPU full-particle sorting/readback determines admission. Unequal/odd runs are zero-padded safely.
- `AestraParticleLightSettings` is an opt-in host resource: `max_lights = 0` by default schedules no
  selection jobs and releases light scratch. The default 64 MiB host GPU-buffer budget is adjustable;
  checked aggregate counter/address/storage/dispatch limits reject unsupported frames explicitly,
  never silently bypass global admission or introduce another fixed emitter/output-count limit.
- Bevy collects compiled output plans and live parameters, reuses GPU scratch/bindings and uploads
  only changed plan/key bytes. Selection runs after actual particle presentation; unpresented owners
  contribute neither stale records nor counters. Jobs bind the same GPU render-transform buffer as
  sprite rendering, including processed-time placement. Light consumers retain position/age/RGB
  through material attribute pruning, including light-only emitters; constant/gradient lights do not
  force otherwise unused particle RGB/size/opacity/rotation work.
- Canonical frame manifests qualify root/owner entities and epochs, nested clip paths, source effect,
  seed/revision, emitter, region and output. Tokens are **frame-local**: asynchronous consumers must
  preserve the corresponding manifest and validate lifecycle epochs. Despawn, disable, invalid live
  gradients and resource rejection cannot reuse an old selected set.
- Native global conformance covers one through 65 input runs, quality/global caps, priority and stable
  cross-output ties, live -> empty -> reordered runs and exact aggregate counters. A headless Bevy
  playback-only probe verifies analytic placement/refresh and lifecycle handling. Two authored peony
  launch-to-star event chains verify actual stateful event-born GPU presentation: **512 requested /
  512 candidates / 48 selected / 464 budget-dropped**, with 32 per star output and 37,056 bytes of
  selection ping-pong scratch. This probe deliberately omits flash/smoke/material/trail draws;
  it is not the full hero/F6 workload or a performance/visual acceptance.
- [Integration evidence and host API](../../benchmarks/fireworks/particle-light-live-integration-2026-10-03.md)
  records the native checks and remaining limits. `GpuSelectedParticleLights` exposes GPU buffers and
  the matching manifest **in the render world**, not realized scene lights or a main-world mailbox.
  There is no production synchronous GPU wait, full-particle transfer or replay dependency.

**F7D2B implemented (2026-10-04): current authored-workload selection and resource measurements.**

- The explicit viewer benchmark adds deterministic generic scene-output fixtures without modifying
  saved effects, material/trail presentation, event chains, particle/history budgets or choreography.
  Hero/overlapping-volley/F6 show run at all three tiers, with selection-disabled global-cap-zero
  baselines. Shader startup is gated before forward playback; 60 Hz benchmark ticks use no replay
  history and disable editor catch-up pacing. Normal interactive behavior remains unchanged.
- Three reusable asynchronous slots copy only 16 counter bytes each, never source/selected particle
  records. A four-result mailbox preserves origin tick/sequence tags and reports busy/overwrite losses.
  GPU diagnostics record selection and one outer render-graph timestamp window, **not** application
  frame time. Timing distributions are independent of counter observations; do not sum percentiles or
  infer incremental cost by subtracting independently sampled baseline percentiles.
- On RTX 4070 SUPER/Vulkan, first show selection p50/p95/p99 was **0.680/1.107/1.245 ms** at high;
  three high runs gave p95 **1.096–1.107 ms**. Medium/low varied substantially, retained in evidence,
  so there is no certified tier speed ordering or production frame budget. High render-window p95 was
  6.929 ms versus 5.802 ms in its independent selection-disabled baseline.
- At peak show candidates, high/medium/low counters were **1536/1536/96/1440**,
  **576/576/48/528**, and **192/192/24/168** (requested/candidate/selected/budget-dropped).
  Up to 31 source runs reserved **314,960/111,888/54,064 bytes**, respectively. All sets obeyed
  host/frame caps and counter algebra; no outputs were rejected. One repeated medium run explicitly
  skipped a busy readback slot rather than waiting. Per-output caps still apply before global admission.
- Both enabled/disabled shows and two additional enabled shows per tier pass the strict 13-clip
  forward admission/cleanup gate: exact 8413/3317/1217 admitted children, no overflow/omission/rejection,
  no trail evictions/truncation, and zero final particles/history/clip owners. A zero-memory-budget
  probe reports explicit rejection and publishes no selected frame. The generic dense-pool F7D1
  conformance remains separate; this fixture is the **current bounded show**, not production-density
  finale acceptance.
- [Workload evidence, provenance and reproduction](../../benchmarks/fireworks/particle-light-workloads-2026-10-04.md),
  [machine-readable measurements](../../benchmarks/fireworks/particle-light-workloads-2026-10-04.json), and
  `benchmarks/fireworks/validate-particle-lights.ps1` capture this gate. Selected buffers remain
  render-world data, not realized lights or an editor viewport-lighting implementation.

**F7E portable realization baseline is now implemented below.** It preserves bounded GPU selection
and playback-only operation; fast-star perceptual/display-lag acceptance remains an explicit next gate.
F7D's production-finale resource/performance certification remains open; selection measurements
alone do not complete F7's visible-lighting or lit-smoke gates.

**Goal:** turn thousands of luminous particles into a small bounded selected-light set entirely on the
presentation side.

#### Inputs already available

The current 48-byte `GpuParticle` presentation ABI already provides:

```text
color
position
size
normalized_age
emitter/alive
particle_index
```

Use these first. `particle_index` is the stable spawn ordinal and is suitable for deterministic
tie-breaking.

#### GPU passes

Implement a measured pipeline such as:

```text
candidate evaluation
    ↓
compaction
    ↓
importance key generation
    ↓
stable top-K / bounded selection
    ↓
selected ParticleLight records
```

Do not append every source particle to a CPU message queue.

Candidate record should stay compact, for example conceptually:

```text
position.xyz + range
color.rgb + intensity
radius + stable identity/flags as required
```

#### Selection requirements

- hard configured maximum;
- deterministic tie-break by stable particle identity;
- camera-aware importance is allowed because this is presentation-only;
- no changes to simulation result/checkpoints;
- explicit requested/candidate/selected/dropped counters;
- offscreen/distant particles may be deprioritized;
- no shadow allocation in this phase.

#### Acceptance workloads

1. one hero shell with hundreds of stars;
2. overlapping volley;
3. F6 full show/finale segment;
4. generic many-luminous-particles probe (e.g. candles/embers) to ensure the feature is not firework-specific.

### F7E — Bevy realization of selected particle lights

**Goal:** make the bounded selected set illuminate ordinary Bevy scene geometry.

**F7E1 implemented — 2026-10-04: opt-in portable baseline, not final perceptual/finale certification.**

- `AestraParticleLightPlugin` realizes only the capped GPU-selected prefix, using bounded asynchronous
  staging, a one-result mailbox and a reusable shadowless `PointLight` pool. Defaults: three slots,
  1 MiB staging, 1 MiB estimated manifest payload per snapshot, 100 ms / eight frames maximum age.
  No source-particle map, per-star entity allocation, CPU event fanout, replay dependency or GPU wait.
- Frame-local source tokens retain their matching manifest and originating compiled artifact.
  Current root/owner epochs, seed, revision, artifact, backend, clip context, enabled output and layers
  are checked before realization. Configuration-qualified packets cannot revive old-budget lights
  while a pipelined render world catches up. Out-of-order completions and expired results fail closed.
- Host caps/clamps remain independent of authored quality: global selection, transport-prefix count,
  selection/staging/manifest byte budgets, result age/lag, lumens and range. Radius is clamped to range;
  both shadow types remain off. Zero global cap removes selected jobs, copies and pooled entities;
  mapped slots drain safely. Idle coordinator systems remain installed.
- The generic native receiver probe uses light-only moving particles (no rendered sprites, bloom,
  ambient light or flashes in its images). It verifies world positions without double transforms,
  a moving receiver response, prefix cap, separate flash coexistence, restart, disable/re-enable,
  byte-budget rejection/recovery, pool resizing and owner removal. Unit tests also cover artifact
  replacement, seed/revision/backend changes, age/frame expiry, layers, clamps and deleted proxies.
- Full authored F6 show, high/medium/low: caps 96/48/24 hold across all thirteen clips; staging stays
  bounded, flash and particle pools coexist, all event admissions remain intact, and final active
  lights/pending maps/staging bytes return to zero. Empty scene pools keep capped idle proxy entities
  for reuse; explicit global disable removes them. Copy diagnostics are separately instrumented.
- [Profiling and reproduction](../../benchmarks/fireworks/particle-light-realization-2026-10-04.md),
  [measurements](../../benchmarks/fireworks/particle-light-realization-2026-10-04.json) and
  `benchmarks/fireworks/validate-particle-light-realization.ps1` record the baseline gate.

**F7E2 measured — 2026-10-04: flagship fast-star registration gate failed.** A paced 60 Hz native
probe keeps Bevy pipelining enabled and measures a red HDR GPU star and green PBR receiver spot
within the same final image. A synchronized test-only control registers within 0.112 pixel. At
25/75/150 m/s, the async path's p95 offset is 0.848/2.576/5.130 metres (approximately two frames,
34 ms). The explicit initial budget is at most one 60 Hz frame and one-quarter of light range;
all three speeds fail. This is an isolated one-star/one-light measurement, not full-show cost,
monitor scanout latency, production-finale certification or human approval of the artwork.
Cadence, native backend, source motion, valid image signals and bounded transport/liveness are
validated separately; a passing measurement test does not imply visual acceptance.
- [Method, result and reproduction](../../benchmarks/fireworks/particle-light-latency-2026-10-04.md),
  [measurements](../../benchmarks/fireworks/particle-light-latency-2026-10-04.json) and
  `benchmarks/fireworks/validate-particle-light-latency.ps1` retain this gate. Without `-MeasureOnly`,
  the validator intentionally fails the current async adapter's flagship registration budget.

**Next: F7E3 — narrow same-frame GPU lighting proof.** The measured visible lag now justifies a
Bevy render-world prototype consuming the selected GPU records without the main-world round trip.
First pass this same fast-star/PBR registration gate with one selected light; then integrate bounded
clustered lighting and remeasure authored hero/volley/show cost, layers and lifecycle. Keep this
adapter-specific and opt-in. Do not block on GPU readback, relax the gate or silently extrapolate.
The portable async pool remains available for latency-tolerant workloads. Production-finale
certification, additional hardware/cadences, editor lighting and lit smoke remain open.

#### Validation path

The first adapter may reuse the existing pooled, shadowless `PointLight` realization **only after GPU
selection**. For GPU simulation, use asynchronous bounded readback of selected records; never read back
the complete particle set and never stall the render thread for current-frame data.

Measure:

- update/readback latency;
- fast-moving-light visual lag;
- CPU cost;
- GPU copy/selection cost;
- pool update cost;
- active/allocated light count.

If this path is visually and computationally acceptable at the chosen budgets, it can remain as a
portable baseline.

#### Production path if required

If readback/pool latency or CPU cost is a measured blocker, add a Bevy render-world integration that
consumes the selected GPU light buffer directly in/near the clustered-light path.

Keep this adapter-specific. Core remains engine-neutral.

#### Acceptance gate

- selected moving stars visibly illuminate nearby geometry;
- no one-entity-per-particle behavior exists;
- hard budgets hold during the full show;
- representative flash light and particle lights coexist;
- global host disable removes particle-light cost/realization;
- profiling evidence is committed under `benchmarks/fireworks/`.

### F7F — lighting quality tiers and optional shadow policy

**Goal:** make lighting scalable rather than all-or-nothing.

Quality tiers should independently control:

```text
representative light pool
particle-light selected count
particle-light max range/intensity clamps
particle-light enable/disable
optional tiny shadowed-light budget (future)
```

Default particle-light shadows remain **off**. If shadow support is added, only a very small selected
set should be eligible and it must be independently budgeted/profilable.

### F7 deliverable

A hero firework should ultimately combine:

```text
all stars       → HDR sprite + trail
main break      → one strong representative event light
selected stars  → bounded moving particle lights
```

with a host-controlled quality policy and measured finale behavior.

Lit particle/volume smoke remains an F7/F8 interaction gate; F7's direct particle-light path does not
by itself make current unlit particle smoke respond to scene lights.

## Milestone F8 — Smoke upgrade

**Goal:** move from acceptable show smoke to high-end persistent smoke.

### F8.1 particle smoke

Ship first if not already sufficient.

### F8.2 particle/event → fluid-domain injection

Design generic coupling for injecting fluid resources at particle/event positions.

### F8.3 lighting interaction

Ensure both relevant VFX lighting paths can affect smoke through the chosen volume-lighting model:

- representative burst flashes must illuminate accumulated smoke;
- selected particle lights may contribute locally when supported/budgeted;
- particle-smoke sprites need an explicitly lit material path or a documented decision to keep them unlit;
- fluid volume lighting must not require CPU enumeration of every particle light.

### Deliverable

Repeated shells build a persistent, moving smoke layer that later shells illuminate.

---

## Milestone F9 — Finale optimization & quality tiers

**Goal:** tune and certify production-level scalability already established by F1/F1B and tested throughout shell development. F9 must not be the first time event/trail limits are addressed.

### Tasks

- benchmark all reference scenes;
- identify dominant GPU passes;
- optimize only measured bottlenecks;
- implement relevant quality-tier scaling;
- refine overflow/budget diagnostics already delivered by F1/F1B;
- validate culling;
- validate off-screen shell behavior;
- validate seeking under maximum density;
- validate multiple simultaneous nested effects.

### Deliverable

A repeatable finale benchmark and documented hardware/profile results.

---

# 23. Suggested priority order

The shortest path to a truthful production-scale showcase is:

```text
F0  validation scene + limit/scale baselines
 ↓
F1  scalable event pipeline + event-link editor
 ↓
F1B scalable trail histories + hero/volley gates
 ↓
F2  velocity distributions
 ↓
F3  Peony + Chrysanthemum + Pistil + Willow
 ↓
F4  HDR + bloom
 ↓
F5  Crossette + Crackle + multi-break
 ↓
F6  complete show
 ↓
F7B2 representative-light editor authoring
 ↓
F7C/F7D bounded direct particle-light output + GPU selection
 ↓
F7E/F7F Bevy realization + lighting quality tiers
 ↓
F8  advanced smoke / fluid coupling
 ↓
F9  finale tier tuning and certification
```

**Current execution priority (2026-10-04):** F7A/F7B1, F7B2 authoring controls, F7C's source/compiler/artifact/CPU contract, F7D1's measured per-output GPU prototype, F7D2A's global admission/live GPU integration and F7D2B's current authored hero/volley/show selection measurements are implemented; F7B2's manual UI/visual gate remains open. Next implement and measure F7E's bounded asynchronous selected-light realization before committing to a deep Bevy render-pipeline integration. The current bounded-show measurements are not production-finale or visible selected-light certification. Representative burst lights already provide a valid full-show lighting baseline, so direct particle lights are a realism/scalability enhancement rather than a reason to block show authoring.

**Do not put fluids before the first full show.**

**Do not put advanced age triggers before an actual shell demonstrates the need.**

**Build small visual prototypes early, but do not call Peony/Chrysanthemum production-ready or lower their target counts until the generic event and trail scale gates pass.** Avoid unrelated runtime redesign; the measured event/trail replacements are required foundation work.

---

# 24. Likely code areas affected

This is not an exhaustive implementation diff, but based on the current repository the work will likely touch these areas.

## Event model/runtime

```text
crates/aestra-core/src/model.rs
crates/aestra-runtime/src/lib.rs
crates/aestra-runtime/src/stateful.rs
crates/aestra-compiler/src/lib.rs
crates/aestra-gpu/
bevy/aestra-bevy-render/src/execution.rs
bevy/aestra-bevy-render/src/gpu.rs
```

The current event gather kernel is embedded in `crates/aestra-gpu/src/lib.rs`; its fixed buffer/list setup is in Bevy's `execution.rs` and `gpu.rs`. Change them as one pipeline, including destination-slot accounting.

## Trail scale/runtime

```text
crates/aestra-core/src/model.rs
crates/aestra-compiler/src/lib.rs
crates/aestra-gpu/src/lib.rs
crates/aestra-gpu/src/shaders/aestra_trail_history.wesl
crates/aestra-gpu/src/shaders/aestra_trail_bounds.wesl
crates/aestra-gpu/src/shaders/aestra_trail_compact.wesl
bevy/aestra-bevy-render/src/gpu.rs
bevy/aestra-bevy-render/src/execution.rs
```

Also inspect trail checkpoint serialization/storage and the draw-order contract before changing ownership or compaction.

## Event authoring/editor

```text
apps/aestra-editor/src/session.rs
apps/aestra-editor/src/properties.rs
apps/aestra-editor/src/properties/inspector.rs
apps/aestra-editor/locales/en-US/editor.ftl
apps/aestra-editor/locales/fr-FR/editor.ftl
crates/aestra-authoring/src/command.rs
crates/aestra-authoring/src/executor.rs
crates/aestra-authoring/src/selection.rs
```

## Distribution system

Likely:

```text
crates/aestra-core/src/model.rs
crates/aestra-compiler/src/lib.rs
crates/aestra-runtime/src/lib.rs
crates/aestra-gpu/src/
apps/aestra-editor/src/properties/
```

plus authored format/migration and contract tests.

## Preview rendering/post-process

Likely:

```text
apps/aestra-editor/
apps/aestra-viewer/
bevy/aestra-bevy-render/
crates/aestra-gpu/src/material.rs
```

Exact ownership should be decided after tracing the preview camera/render graph when implementing F4.

## Scene lighting / particle-light outputs

Current representative path:

```text
crates/aestra-core/src/scene_outputs.rs
crates/aestra-core/src/model.rs
crates/aestra-runtime/src/scene_outputs.rs
crates/aestra-compiler/src/lib.rs
crates/aestra-artifact/src/lib.rs
bevy/aestra-bevy/src/lights.rs
bevy/aestra-bevy/README.md
apps/aestra-viewer/
```

Direct particle-light path will additionally touch likely:

```text
crates/aestra-core/src/model.rs                 # emitter scene-output model
crates/aestra-runtime/src/lib.rs                # compiled output plan / CPU reference
crates/aestra-compiler/src/lib.rs               # lowering + parameter discovery
crates/aestra-gpu/src/lib.rs                    # candidate/selection kernels
bevy/aestra-bevy-render/src/gpu.rs              # GPU resources/passes
bevy/aestra-bevy-render/src/execution.rs        # bindings/capacity/planning
bevy/aestra-bevy/src/lights.rs                  # selected-set realization baseline
apps/aestra-editor/src/properties/              # authoring
apps/aestra-viewer/                             # benchmark/acceptance scene
```

Before adding `RendererProperties::Light`, remember that current `RendererInstance`, `RendererPlan`
and `CompiledExtensionRenderer` all structurally carry a material. Prefer a sibling material-free
scene-output model unless a broader renderer-contract migration is explicitly justified.

## Fluid coupling

```text
extensions/aestra-fluid/
```

plus whichever generic resource/domain bridge belongs in core/runtime.

---

# 25. Test plan

Every new generic primitive introduced for fireworks should have coverage at multiple layers.

## Core/model tests

- validation ranges;
- serialization;
- migration;
- semantic IDs;
- defaults.

## Authoring tests

- command execution;
- undo/redo;
- selection;
- property editing;
- save/reload.

## Compiler tests

- core → compiled representation;
- deterministic seed propagation;
- quality tier lowering;
- invalid configuration diagnostics.
- checked demand/capacity calculations, multi-link fan-out and device-budget rejection;
- a 300–800-parent trail emitter that previously failed validation;
- budget estimates including retired tails and checkpoint storage.

## CPU reference tests

Where applicable:

- distribution sampling;
- event semantics;
- deterministic ordering.

## GPU conformance tests

- CPU/GPU sample parity;
- event chains;
- host cue identity/delivery across clip instances, checkpoint restore and seek;
- large bursts;
- trail state;
- replay after checkpoints;
- seek forward/backward;
- overflow handling.
- same-tick high-fan-out event ordering across multiple workgroups/chunks;
- exact requested/emitted/dropped accounting at source, expansion and target;
- hundreds of live trail owners plus retained retired tails, including eviction;
- deterministic history ownership, geometry and alpha order after compaction and seek.

## Visual regression tests

Reference captures for:

```text
peony
chrysanthemum
willow
ring
crossette
full show frame(s)
```

Post-process settings and seeds must be fixed.

## Performance tests

Track over time rather than relying on one-off profiler inspection.

Record:

```text
GPU simulation ms
GPU event ms
trail update ms
trail render ms
sprite render ms
post-process ms
fluid/smoke ms
VRAM / buffers
checkpoint memory
seek latency
```

Run the Test A/B/C ladder from section 21 on named hardware at stated resolution, quality tier, memory budget and target frame time. Report compile rejection separately from runtime timing, and record requested/produced work so a faster result cannot be caused by silent drops.

---

# 26. Definition of done for the fireworks showcase

The feature is not complete merely because a screenshot looks good.

Aestra should satisfy all of the following.

## Authoring

- A user can create shell hierarchy through the editor.
- Event links are editable and understandable.
- Complex shells are reusable assets.
- Shell parameters are overrideable per `EffectClip`.
- Different shell types require no engine code.
- A show can be arranged primarily on the timeline.

## Simulation

- Peony, Chrysanthemum, Willow, Pistil, Crossette, Crackle, Palm and Ring are representable.
- Particle event chains work on GPU.
- High burst counts do not require duplicate-link hacks.
- Trails survive/replay correctly.
- Fixed seeds are deterministic.
- Backward seeking works acceptably.
- A single hero emitter and link meet the section 6/8 scale gates without graph duplication to bypass implementation ceilings.
- Runtime pressure has explicit deterministic behavior and complete requested/emitted/dropped telemetry.

## Rendering

- bright stars use HDR values;
- bloom and exposure are controllable;
- additive overlap feels luminous;
- long trails remain smooth;
- shell can illuminate reference geometry;
- smoke exists and persists at show scale.

## Performance

- normal volleys remain interactive;
- finale workload has defined budgets;
- overflow is observable, never silently ignored;
- quality tiers have meaningful firework-related scaling;
- profiling tells the author why a show is expensive.
- the named Test A/B/C workloads compile and execute at their stated tier/hardware budgets, or the asset displays a clear budget failure instead of silently losing authored work.

## Extensibility

- distribution model is generic;
- scene-light output is engine independent;
- advanced smoke can use an extension/domain bridge;
- no core concept is named after fireworks.

---

# 27. Current readiness assessment

These are not project-completion percentages. They represent readiness specifically for the realistic-fireworks objective.

```text
Core particle engine          █████████░   very strong
Stateful simulation           █████████░   very strong
Seeking/checkpoints           ████████░░   strong
Trail feature/quality         █████████░   strong foundation
Trail hero/finale scale       █████░░░░░   paged volley proven; budget/finale gate open
Multiple renderers            █████████░   ready
Particle event semantics      ████████░░   bounded workloads
Event hero/finale scale       ██░░░░░░░░   blocked by fixed queues/gather
Particle event authoring      ████░░░░░░   needs work
Host simulation cue output    ██░░░░░░░░   needed for sound binding
Effect composition/timeline   ████████░░   strong
Spawn position shapes         ████████░░   good
Velocity distributions        ████░░░░░░   main sim gap
HDR/post-process preview      █████░░░░░   infrastructure, UX/pipeline gap
Environmental VFX lighting    ███░░░░░░░   missing
Particle smoke                ██████░░░░   feasible now
Volumetric smoke foundation   ███████░░░   advanced extension exists
Particle→fluid coupling       ███░░░░░░░   missing
Show-specific content         ██░░░░░░░░   not yet built
Finale validation/LOD         ███░░░░░░░   needs benchmark
```

---

# 28. Immediate next sprint recommendation

If only one focused implementation cycle is available, do this:

### 1. Reproduce and instrument the limits

- Add Test A as a compile-and-run regression fixture plus isolated same-tick event and live/retired trail microbenchmarks.
- Capture exact current failures/truncation, GPU time, memory and replay/seek behavior on named hardware.

### 2. Deliver F1's event scale gate

- Replace the fixed gather/list path with deterministic scalable ordering and expansion.
- Account for target capacity and make every overflow observable.
- Expose count and inherited velocity in the editor; then allow a high-count link to pass the complete pipeline.

### 3. Deliver F1B's trail scale gate

- Ownership/history/bounds/compaction are parallelized; paged pools and checked adapter-resource preflight are implemented. Native tests cover one 8,192-star emitter with 16,384 owners without splitting its trail graph.
- Correctly budgeted overlapping cohorts and retained tails now have loss-free native regressions and measured phase costs. Next, optimize the dominant paged-history work and certify the named live/full-frame budget. Physical-buffer chunking and the 1,048,576-record ceiling remain open.

### 4. Continue visual authoring in parallel

Sketch Peony and Chrysanthemum at low counts to test UX. After the scale gates, bring them to Test A density; then add deterministic velocity distributions and HDR/bloom before judging visual realism.

At the end of the cycle, compare requested versus produced particles/trails and measured budget results. Do not count a small prototype as evidence that the AAA workload is feature-ready.

---

# 29. Final conclusion

Aestra has crossed an important threshold.

Previously, a realistic fireworks show required foundational systems that did not yet exist. In the current codebase, the key foundations are present:

```text
stateful particles
+ deterministic replay
+ GPU event links
+ nested sub-emission
+ multiple renderers
+ real trail history
+ timeline/effect composition
+ extensible stages/domains
```

The project is ready to use fireworks as a **reference production workload**, not yet to claim that the workload is supported at AAA density.

The first engine blockers are **scalable deterministic events and trails**. The next generic authoring feature is richer initial velocity/distribution control. A host-facing simulation cue API is needed for sound binding without putting audio playback in Aestra. The most important visual gap is HDR/bloom/exposure, followed by scene lighting and later persistent illuminated smoke.

The correct development strategy is now:

> benchmark the target workloads first; fix event and trail scaling as generic Aestra capabilities; build production-density shells only after those gates pass; then compose and optimize the show using measured visual and performance evidence.

That gives Aestra both a compelling showcase and a demanding integration test for the next-generation VFX architecture.


---

# 26. 2026-10-03 focused code-review conclusions: lighting

This section records the conclusions from the uploaded current project snapshot so future roadmap
edits do not regress to the older assumption that VFX lighting is absent.

## Confirmed in code

1. `EffectAsset` contains `point_lights: Vec<PointLightBinding>`.
2. `PointLightBinding` targets a stable particle-output route and validates `FirstPerTick` aggregation.
3. `PointLightPulse` has normalized RGB, lumen/range curves, radius and independent duration.
4. Runtime resolves compiled bindings into `TransientPointLight` intents at world-space particle-event positions.
5. `AestraTransientLightPlugin` owns a bounded/reused pool of shadowless Bevy `PointLight` entities.
6. The adapter has explicit global budgets/clamps and admission/drop telemetry.
7. Firework benchmark evidence demonstrates bounded full-show admission and ordinary diffuse receiver response.
8. Current particle smoke remains unlit.
9. Current authored light binding intentionally does not support `EachEvent`/arbitrary per-particle lights.
10. Current `RendererInstance` and extension renderer structures require a material, so a direct light output does not cleanly fit as an ordinary renderer without a contract change.
11. The GPU particle presentation record already exposes enough state for an initial direct-light selector: color, position, size, normalized age, alive/emitter packing and stable particle index.
12. Stateful simulation keeps stable spawn ordinals, so presentation selection can use deterministic particle identity without changing simulation state.

## Resulting architectural decision

Keep:

```text
PointLightBinding
= sparse event-triggered representative light
```

Add:

```text
Particle Point Light Scene Output
= bounded continuous presentation derived from alive particles
```

Do not make the second one an `EachEvent` host-message flood, and do not replace the first one.

## Firework lighting composition target

```text
                         FIREWORK

all visible stars ────────────────> HDR/bloom
       │
       ├──────────────────────────> trails
       │
       └─ selected bounded subset ─> moving particle lights

main burst event ─────────────────> representative transient flash
```

This architecture also generalizes to embers, fireflies, magic/projectile VFX and large flame fields.

## Immediate next work

```text
1. F7E3 same-frame selected GPU lighting proof; F7E2 measured async lag fails the fast-star gate
2. F7F   quality tiers; shadows remain off by default
3. F8.3  make smoke actually consume scene lighting
```

F7B2 editor controls are implemented; complete their manual UI/visual acceptance alongside source
authoring, without treating them as evidence of continuous particle lights or editor viewport lighting.
F7C's material-free plans/CPU reference, F7D1's measured per-output GPU selection, F7D2A's
global admission/live GPU wiring and F7D2B's current authored hero/volley/show selection measurements
are implemented. F7E1's bounded async realization is implemented; F7E2's paced receiver/HDR-star
measurement fails the initial fast-star registration budget. Production-finale certification and
same-frame GPU realization remain open.

The generic model, bounded selection and portable receiver contribution are now proven. F7E2's
measured approximately two-frame visible lag justifies a narrow backend-specific GPU receiver proof,
before committing to full clustered-light integration. Preserve the portable path and core/adapter
boundary; no blocking readback or unbounded source-particle transport.
