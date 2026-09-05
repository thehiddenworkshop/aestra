# GPU Particle Architecture — How Aestra Compares to Professional VFX Engines

A comparative study of the GPU simulation architectures used by established
real-time VFX engines — **bevy_hanabi**, **Unreal Niagara**, **Unity VFX Graph**,
and **Wicked Engine** — and what each of their optimization techniques means for
Aestra.

Companion to the benchmarking work in
[`aestra_runtime_benchmarking_implementation_plan.md`](aestra_runtime_benchmarking_implementation_plan.md)
(see Phase 6's scaling verdict and Phase 7's per-particle floor) and
[`aestra_gpu_architecture_portability.md`](aestra_gpu_architecture_portability.md).

> **Why this exists.** Phase 7 profiling proved the analytical GPU kernel is near
> its practical floor: ~67% of per-particle cost is inherent full-frame
> reconstruction that no micro-optimization touches. Before committing to a large
> architectural change (strategy M7), it is worth knowing exactly what the
> professional engines do differently — and which of their techniques transfer to
> Aestra *without* discarding what makes Aestra's design valuable.

---

## 1. The central architectural divide

Every one of these engines shares one design choice, and it is the **opposite** of
Aestra's:

| | Professional engines (Hanabi, Niagara, Unity VFX Graph, Wicked) | Aestra |
| --- | --- | --- |
| **Model** | **Stateful / incremental** | **Stateless / analytical** |
| **Per frame** | Integrate *one delta* (Euler step) in place | Reconstruct *every slot from t=0* |
| **Particle state** | Persists in a GPU buffer across frames | None — recomputed each frame |
| **Work per frame** | Proportional to *alive* particles | Proportional to *capacity* |
| **Seek to arbitrary time** | Impossible — must simulate forward frame-by-frame | Direct — jump to any `t` |
| **Determinism across runs/machines** | Weak (accumulated float drift) | Bit-exact by construction |

This single difference explains the Phase 7 finding. Aestra pays to rebuild the
world every frame precisely because it refuses to keep state — and refusing to keep
state is what buys deterministic seeking and reproducibility, which are exactly the
properties an **authoring / choreography tool** needs (timeline scrubbing, reference
tests, machine-independent snapshots).

The professional engines are optimized for *playback throughput*. Aestra is
optimized for *authoring correctness*. Neither is wrong; they are different points
on the same trade curve.

---

## 2. The shared "GPU-driven" playbook

The stateful engines converge on the same pipeline (Wicked Engine documents it most
openly; Niagara, Unity VFX Graph, and Hanabi are variations on it):

| Technique | What it does | In Aestra today |
| --- | --- | --- |
| **Persistent buffer + update-in-place** | Integrate a delta, don't rebuild | ✗ full reconstruction each frame |
| **Dead / free list** | A stack of free slot indices; emit *pops*, death *pushes back* (atomic stack pointer) | ✗ each slot maps to a spawn event analytically |
| **Alive lists (ping-ponged)** | Two index lists; simulate reads list A, writes survivors to list B — process only alive, not capacity | ✗ dispatch covers full capacity |
| **Indirect dispatch** (`DispatchIndirect`) | A tiny "kickoff" pass reads the alive counter and writes the threadgroup count, so the GPU dispatches `ceil(alive/64)` groups | ✗ always dispatches `ceil(capacity/64)` |
| **Indirect draw** (`DrawIndexedIndirect`) | Sim writes instance-count into draw args; no CPU readback | partial |
| **GPU-driven emission** | Spawn count + slot allocation entirely on GPU | ✗ CPU-side |
| **SoA layout + FP16 attributes** | Structure-of-arrays so a pass touches only the fields it reads; half-precision where tolerable — pure bandwidth win | ✗ (attacks the 67% memory floor) |
| **Bitonic sort on the alive list** | Camera-distance sort for correct alpha / additive blending (AMD's implementation is the industry standard) | orthogonal, not done |
| **Attribute reduction** | Compile the emitter so only *referenced* attributes are allocated / processed | partial |
| **LOD / culling / scalability** | Distance-scaled particle counts; cull off-screen emitters; GPU frustum culling | orthogonal |
| **Simulation stages / grids** | Niagara Grid2D/3D, neighbor grids, iterative multi-pass solvers | out of scope |

The two with the largest throughput impact are **indirect dispatch** (do work
proportional to *alive*, not *capacity*) and **update-in-place** (integrate a delta
instead of rebuilding).

---

## 3. Per-engine notes

### bevy_hanabi
A modern GPU-first particle system for Bevy: "offloading most of the work to the
GPU, with minimal CPU intervention." Stateful — persistent per-particle attributes
(position, velocity, color, size, age, lifetime) updated each frame through
modifiers, fixed-capacity buffers, spawn modes (constant rate / one-shot burst /
repeated burst). Uses the dead-list + indirect-dispatch pattern and render-layer
support implying conditional/indirect rendering. The closest architectural analogue
to a potential Aestra M7 backend, and worth reading as a reference implementation
in Rust/wgpu specifically.

### Unreal Niagara
A general-purpose GPU compute framework "that happens to be very good at
particles." Key ideas beyond the shared playbook:
- **Simulation Stages** — arbitrary multi-pass compute per emitter, over particles
  *or* over grids (Grid2D/Grid3D) for fluids, neighbor queries, iterative solvers.
- **Deep dispatch profiling** — per-stage threadgroup counts and timing exposed in
  Unreal Insights / RenderDoc / PIX; the guidance is explicitly "reduce emitters =
  fewer dispatches per frame," which matches Aestra's own §2.3 emitter-lookup
  finding (cost bites at high emitter counts).
- **Resolution/scalability trade** — e.g. a 1024² grid stage costing 1–2 ms is
  dropped to 512² when visually indistinguishable. The engine leans hard on "only
  pay for what you use": attributes not referenced are not allocated.

### Unity VFX Graph
Entirely GPU-based (unlike the legacy CPU Shuriken system): "each block is a compute
shader," driving hundreds of thousands to millions of particles. Requires compute
support (silently fails without it — a portability note for Aestra's own backend
matrix). Same persistent-buffer + compute-block model; the graph compiles to a
sequence of compute passes over a persistent attribute buffer.

### Wicked Engine
The most openly documented reference for the canonical pipeline: a **dead list**
(free indices), **two alive lists** ping-ponged across frames, a **counter buffer**,
and a sequence of compute passes — *kickoff* (writes the `DispatchIndirect` /
`DrawIndirect` args from the counters) → *emit* (pops from the dead list, writes new
particles) → *simulate* (integrates, moves dead particles back to the dead list,
appends survivors to the new alive list) → *indirect draw*. Optional bitonic sort of
the alive list for blending. If Aestra ever builds M7, this is the blueprint.

---

## 4. Technique-by-technique: what transfers to Aestra

Sorting the playbook by whether it fits Aestra's analytical model:

### Transfers *without* abandoning the analytical model
- **SoA + FP16 attribute packing.** The Phase 7 floor is ~67% memory + reconstruction.
  Structure-of-arrays output and half-precision on tolerant attributes attack the
  memory half directly, with no change to determinism or seeking. Bounded, measurable.
- **Bitonic sort for correct blending.** Needed for correct alpha/additive order
  regardless of backend; orthogonal to the state model. A candidate whenever
  blend-order artifacts matter.
- **LOD / off-screen emitter culling / scalability tiers.** Reduce *capacity* (and
  thus dispatch size) for distant or minor effects — and because Aestra dispatches
  over capacity, cutting capacity is a direct, proportional win here.

### The idea that looked like the standout — and why measurement killed it
There *appears* to be a **third option** neither camp uses, available to Aestra
because it is analytical:

> Aestra can compute the alive-particle count for any time `t` on the CPU cheaply
> (it already inverts the emission curve — Phase 7 #2). So it *could* size the
> compute dispatch to the known alive count — `ceil(alive/64)` groups — with no
> GPU-side dead list and no loss of determinism, capturing the professional
> engines' highest-impact technique (work ∝ alive, not capacity) while staying
> seekable.

Attractive on paper. **The measurement and the kernel's slot layout jointly refute
it** — recorded here so it is not re-proposed on intuition:

1. **The measured cost is already at the timer floor.** The archived GPU baseline
   ([`benchmarks/gpu-baselines/20d5785…/b004_sparse_large.json`](../benchmarks/gpu-baselines/20d57855763fb90cbe7fac2bd8eb8dab731bc1f5/b004_sparse_large.json))
   puts b004's `simulate` at **p50 0.001024 ms — one GPU timer tick** (min 0.0),
   while b004's own transparent render pass is **0.043 ms (~40×)**. A dead slot's
   `particle_index >= emitted` early-return is a couple of instructions the GPU
   hides behind occupancy, so Aestra's analytical kernel *already* gets the
   indirect-dispatch benefit for free on this GPU. This is the §2.2 refutation
   extended to the sparse case — not a new opportunity.

2. **The slot layout defeats it for *multi*-emitter effects anyway.** Emitters are
   packed contiguously by `slot_offset`: emitter *i* owns
   `[slot_offset_i, slot_offset_i + max_particles_i)`. A sparse effect with *N*
   emitters (capacity *C* each, occupancy *o ≪ C*) puts the last emitter's live
   slots at the *top* of the range — `slot_offset_{N-1} + o ≈ (N-1)·C + o`. A single
   global dispatch sized to that high-water mark still covers ~`total_slots`, saving
   only `(C − o)` out of `N·C` — **under 2% at 64 emitters.** It works fully only for
   `N = 1` (b004), the exact case point 1 shows is unmeasurable.

**Verdict:** the single-dispatch analytically-sized variant is not a justified
optimization on measured hardware. The version that *is* both measurably motivated
and layout-correct is **per-emitter dispatch** (next section).

### The measurably-motivated version: per-emitter dispatch (= Phase 7 #3)
The real many-emitter cost is the per-slot linear **emitter search** (§2.3):
b006 (64 emitters, dense 100k) measures `simulate` **p50 0.965 ms**, ~2.7× the
single-emitter kernel at the same particle count. Giving each emitter its own
dispatch — Niagara's "fewer emitters = fewer dispatches" model — removes the search
*and*, as a side effect, sizes each dispatch to that emitter's own occupancy, which
finally captures the sparse win that single-dispatch sizing could not. This is the
plan's **Phase 7 #3** (previously "low priority" on the strength of §2.3 biting only
at 64+ emitters). It subsumes the analytically-sized-dispatch idea; build *this* if
the many-emitter path needs to get faster, and measure against b006.

### Requires going stateful — i.e. *is* M7
- **Dead/alive lists, update-in-place, true O(alive) simulation.** These are the
  strategy-M7 incremental backend. They deliver the biggest ceiling but **sacrifice
  seeking and bit-exact determinism**, so in Aestra they can only be a *second,
  optional playback backend* — not a replacement for the analytical kernel, which
  must stay for editor scrubbing, reference evaluation, and snapshot tests. Both
  backends share the semantic effect IR (already the plan's M7/M8 stance).

### Measurement already says *don't bother*
- **Dead-list / atomic removal for performance** — Phase 7 measured atomics at ~5%.
- **Transcendental force micro-opt** (turbulence) — measured ~0 (runs on GPU SFUs).
- **Dead-slot dispatch elimination (dense *and* sparse)** — §2.2 refuted; b004
  `simulate` p50 = 0.001 ms (timer floor). The dead-slot early-return already gives
  the indirect-dispatch benefit for free on the RTX 4070 SUPER; see §4.
- **Single global analytically-sized dispatch** — layout-defeated for multi-emitter,
  unmeasurable for single-emitter; superseded by per-emitter dispatch. See §4.

---

## 5. Re-ranked roadmap (measurement-backed)

1. **SoA + FP16 particle attributes** — attacks the measured 67% memory floor
   (Phase 7) without changing the model. Bounded and measurable — the clearest
   remaining kernel win.
2. **Per-emitter dispatch (Phase 7 #3)** — removes the §2.3 per-slot emitter search
   (b006 = 0.965 ms) and, as a side effect, sizes each dispatch to its emitter's
   occupancy. Only justified once the many-emitter path needs to be faster; measure
   against b006.
3. **M7 incremental / stateful backend** — the full dead-list + alive-list +
   update-in-place playbook, as a *second playback backend* that keeps the analytical
   kernel for authoring. Biggest ceiling, largest effort; gate on whether #1 and #2
   close the gap enough.
4. **Bitonic sort** — adopt when blend-order correctness demands it; orthogonal.

**Not pursued** (measurement/layout refuted): single global analytically-sized
dispatch (§4).

The through-line: Phase 7 profiling said the analytical kernel is near its floor,
and this survey asked whether the professional engines' *indirect, alive-proportional*
execution is the way past it. The seductive answer — CPU-sized analytical dispatch —
does not survive contact with the measured cost (already at the timer floor for the
only case it fits) or the kernel's packed slot layout. What survives is narrower and
honest: **attack the memory floor (SoA/FP16), and reach for per-emitter dispatch only
when the many-emitter search actually hurts.** The big architectural lever remains
M7 — and it, too, waits for a workload the analytical kernel provably cannot serve.

---

## Sources

- [bevy_hanabi](https://github.com/djeedai/bevy_hanabi)
- [Wicked Engine — GPU-based particle simulation](https://wickedengine.net/2017/11/gpu-based-particle-simulation/)
- [Niagara: Simulation Stages, Grid2D, GPU-driven effects (StraySpark)](https://www.strayspark.studio/blog/niagara-vfx-advanced-simulation-stages)
- [Complete Guide to Niagara VFX Optimization (MoreVFX Academy)](https://morevfxacademy.com/complete-guide-to-niagara-vfx-optimization-in-unreal-engine/)
- [Getting Started with Unity VFX Graph (UhiyamaLab)](https://uhiyama-lab.com/en/notes/unity/unity-vfx-graph-guide/)
